//! Differential pair routing in the PCB editor.
//!
//! The pair is routed as **one thick track** (the centre path, `2 × width + gap` wide, so walk
//! around keeps clear of everything for both legs) and the two legs are its parallel offsets
//! (`buturecad_router::diffpair`). Pad stubs join the legs to the pads, a through via is placed
//! as two vias side by side, and the finished route — both nets, tracks and vias — is **one**
//! `PcbEditCommand` validated against the DRC rules, including the gap between the legs.
//!
//! Live numbers (length of each leg, difference, gap) come from the real offset geometry, so
//! corner skew shows up as you route. Length tuning to remove it is roadmap stage 24.

use super::pcb_editor::mm_str;
use super::pcb_route::{Hit, ObstacleMode, Resolved, Route};
use super::*;
use crate::pcb_scene as ps;
use buturecad_eda_core::model::{PcbChange, PcbObject, Track, Via, ViaKind};
use buturecad_eda_core::routing;
use buturecad_geometry::{Circle, Coord, Segment};
use buturecad_pcb::LayerId;
use buturecad_renderer::{Rgba, Shape};
use buturecad_router::{diffpair, RouteCandidate, RouteParams};

/// A differential pair route in progress (see [`Route::pair`]).
#[derive(Clone)]
pub(crate) struct PairRoute {
    pub(super) name: String,
    pub(super) pos: Id,
    pub(super) neg: Id,
    pub(super) width: Coord,
    pub(super) gap: Coord,
    pub(super) max_skew: Coord,
    /// The pads the route started on: positive and negative.
    pub(super) p_pad: Point,
    pub(super) n_pad: Point,
}

/// The copper of a pair: per net, polylines per layer (legs and stubs), and the vias.
pub(super) struct PairCopper {
    pub(super) p: Vec<(LayerId, Vec<Point>)>,
    pub(super) n: Vec<(LayerId, Vec<Point>)>,
    pub(super) p_vias: Vec<Point>,
    pub(super) n_vias: Vec<Point>,
    /// The coupled parts only (no stubs), for the live gap.
    coupled: Vec<(Vec<Point>, Vec<Point>)>,
}

fn mid(a: Point, b: Point) -> Point {
    Point::new((a.x + b.x) / 2, (a.y + b.y) / 2)
}

fn dir(a: Point, b: Point) -> (f64, f64) {
    let (dx, dy) = ((b.x - a.x) as f64, (b.y - a.y) as f64);
    let l = dx.hypot(dy);
    if l == 0.0 {
        (1.0, 0.0)
    } else {
        (dx / l, dy / l)
    }
}

impl Workspace {
    /// Pads of `net` with copper on `layer`, with their centres.
    fn pair_pads_of(&self, net: Id, layer: LayerId) -> Vec<Point> {
        let Some(doc) = self.pcb_doc() else {
            return Vec::new();
        };
        let mut out = Vec::new();
        for f in doc.placed.values() {
            let Some(def) = doc.footprints.get(&f.footprint) else {
                continue;
            };
            for p in f.world_pads(def) {
                if f.pad_nets.get(&p.number) == Some(&net)
                    && p.copper_layers(&doc.stackup).contains(&layer)
                {
                    out.push(p.center);
                }
            }
        }
        out
    }

    /// For a pad of one net of a pair: `(positive pad, negative pad, the pair, the layers)` using
    /// the partner pad nearest to it.
    fn pair_pads_for(&self, hit: &Hit) -> Option<(Point, Point, buturecad_pcb::DiffPair)> {
        let doc = self.pcb_doc()?;
        let net = hit.net?;
        let pair = doc.rules.pair_of(Some(net))?.clone();
        let layer = hit.layers.first().copied()?;
        let other = if pair.positive == net {
            pair.negative
        } else {
            pair.positive
        };
        let partner = self.pair_pads_of(other, layer).into_iter().min_by(|a, b| {
            a.distance(hit.at)
                .partial_cmp(&b.distance(hit.at))
                .unwrap_or(std::cmp::Ordering::Equal)
        })?;
        let (p, n) = if pair.positive == net {
            (hit.at, partner)
        } else {
            (partner, hit.at)
        };
        Some((p, n, pair))
    }

    /// Starts a pair route on a pad of a pair net (the `route_pair` tool).
    pub(super) fn pair_start(&mut self, raw: Point, out: &mut Outcome) {
        let Some(hit) = self.pcb_hit(raw) else {
            self.log(
                MessageLevel::Warn,
                "Click a pad of a differential pair net to start (define pairs in Net classes…)",
            );
            return;
        };
        if hit.what != "pad" {
            self.log(
                MessageLevel::Warn,
                "Start the pair on a pad of one of its nets",
            );
            return;
        }
        let Some((p_pad, n_pad, pair)) = self.pair_pads_for(&hit) else {
            self.log(
                MessageLevel::Warn,
                format!(
                    "Net {} is not in a differential pair, or its partner has no pad on this layer",
                    self.net_name(hit.net)
                ),
            );
            return;
        };
        let (width, gap) = self
            .pcb_doc()
            .map_or((200_000, 200_000), |d| d.rules.pair_dims(&pair));
        let active = self.active_copper();
        let route_layer = if hit.layers.contains(&active) {
            active
        } else {
            hit.layers.first().copied().unwrap_or(active)
        };
        self.active_pcb_layer = route_layer.0;
        let mut highlight = self.net_highlight(pair.positive);
        highlight.extend(self.net_highlight(pair.negative));
        self.pcb_ed.route = Some(Route {
            net: Some(pair.positive),
            layer: route_layer,
            pts: vec![mid(p_pad, n_pad)],
            done: Vec::new(),
            vias: Vec::new(),
            highlight,
            resolved: None,
            pair: Some(PairRoute {
                name: pair.name.clone(),
                pos: pair.positive,
                neg: pair.negative,
                width,
                gap,
                max_skew: pair.max_skew,
                p_pad,
                n_pad,
            }),
        });
        out.redraw = true;
        out.selection_changed = true;
    }

    /// Where the centre path may end: the middle of the two target pads when the pointer is on a
    /// pad of the pair.
    pub(super) fn pair_end(
        &self,
        r: &Route,
        pair: &PairRoute,
        raw: Point,
        snapped: Point,
    ) -> (Point, Option<Hit>) {
        match self.pcb_hit(raw) {
            Some(h)
                if h.what == "pad"
                    && (h.net == Some(pair.pos) || h.net == Some(pair.neg))
                    && h.layers.contains(&r.layer) =>
            {
                match self.pair_pads_for(&h) {
                    Some((p, n, _)) => {
                        let at = mid(p, n);
                        (
                            at,
                            Some(Hit {
                                net: Some(pair.pos),
                                at,
                                layers: h.layers,
                                what: "pad",
                            }),
                        )
                    }
                    None => (snapped, Some(h)),
                }
            }
            other => (snapped, other),
        }
    }

    /// The two target pads `(positive, negative)` under the pointer, when it is on the pair.
    fn pair_target_pads(&self, r: &Route, pair: &PairRoute, raw: Point) -> Option<(Point, Point)> {
        let h = self.pcb_hit(raw)?;
        if h.what != "pad"
            || !(h.net == Some(pair.pos) || h.net == Some(pair.neg))
            || !h.layers.contains(&r.layer)
        {
            return None;
        }
        self.pair_pads_for(&h).map(|(p, n, _)| (p, n))
    }

    pub(super) fn pair_params(&self, r: &Route, pair: &PairRoute) -> RouteParams {
        RouteParams {
            net: Some(pair.pos),
            also_net: Some(pair.neg),
            layer: r.layer,
            width: 2 * pair.width + pair.gap,
        }
    }

    /// Continuation of the centre path from its last point to `end`.
    pub(super) fn pair_resolve(&self, r: &Route, pair: &PairRoute, end: Point) -> Resolved {
        let last = *r.pts.last().unwrap_or(&end);
        let (mode, flip) = (self.pcb_ed.route_mode, self.pcb_ed.route_flip);
        let (leg, note) = match self.pcb_ed.obstacles {
            ObstacleMode::Ignore => (Ok(routing::path(last, end, mode, flip)), None),
            other => (
                self.leg_with(self.pair_params(r, pair), last, end, false),
                (other == ObstacleMode::Push)
                    .then(|| "push & shove is not available for pairs; walking around".into()),
            ),
        };
        Resolved::new(leg, note)
    }

    /// The legs, stubs and vias of the pair for the centre path so far plus `leg`.
    pub(super) fn pair_copper(
        &self,
        r: &Route,
        pair: &PairRoute,
        leg: &[Point],
        target: Option<(Point, Point)>,
    ) -> PairCopper {
        let mut current = r.pts.clone();
        current.extend(leg.iter().skip(1));
        let mut centre: Vec<(LayerId, Vec<Point>)> = r.done.clone();
        let current = routing::simplify(&current);
        let has_current = current.len() >= 2;
        if has_current {
            centre.push((r.layer, current.clone()));
        }
        let half_pitch = (pair.gap + pair.width) as f64 / 2.0;
        // Which leg is the positive one: the side of the first segment where its pad lies.
        let first = centre.first().map(|(_, c)| c.clone()).unwrap_or_default();
        let p_left = match first.as_slice() {
            [a, b, ..] => {
                let d = dir(*a, *b);
                let v = ((pair.p_pad.x - a.x) as f64, (pair.p_pad.y - a.y) as f64);
                d.0 * v.1 - d.1 * v.0 >= 0.0
            }
            _ => true,
        };
        let mut out = PairCopper {
            p: Vec::new(),
            n: Vec::new(),
            p_vias: Vec::new(),
            n_vias: Vec::new(),
            coupled: Vec::new(),
        };
        let mut starts: Vec<(Point, Point)> = Vec::new();
        let mut ends: Vec<(Point, Point)> = Vec::new();
        for (l, c) in &centre {
            let (left, right) = diffpair::coupled(c, pair.width, pair.gap);
            let (pp, nn) = if p_left { (left, right) } else { (right, left) };
            if let (Some(ps), Some(ns), Some(pe), Some(ne)) =
                (pp.first(), nn.first(), pp.last(), nn.last())
            {
                starts.push((*ps, *ns));
                ends.push((*pe, *ne));
            }
            out.coupled.push((pp.clone(), nn.clone()));
            out.p.push((*l, pp));
            out.n.push((*l, nn));
        }
        // Side-by-side vias at each layer change, stubbed to the next layer's legs.
        let side = if p_left { 1.0 } else { -1.0 };
        for (k, v) in r.vias.iter().enumerate() {
            let d = r
                .done
                .iter()
                .find(|(_, c)| c.last() == Some(v))
                .and_then(|(_, c)| c.windows(2).last().map(|w| dir(w[0], w[1])))
                .or_else(|| {
                    r.done
                        .iter()
                        .chain(std::iter::once(&(r.layer, current.clone())))
                        .find(|(_, c)| c.first() == Some(v))
                        .and_then(|(_, c)| c.windows(2).next().map(|w| dir(w[0], w[1])))
                })
                .unwrap_or((1.0, 0.0));
            let normal = (-d.1, d.0);
            let off = |s: f64| {
                Point::new(
                    (v.x as f64 + s * side * half_pitch * normal.0).round() as i64,
                    (v.y as f64 + s * side * half_pitch * normal.1).round() as i64,
                )
            };
            let (pv, nv) = (off(1.0), off(-1.0));
            out.p_vias.push(pv);
            out.n_vias.push(nv);
            // The legs of the segment that follows the via start at their own offsets: join.
            let after = k + 1;
            if let Some((ps, ns)) = starts.get(after) {
                let layer_after = centre.get(after).map(|(l, _)| *l);
                if let Some(l) = layer_after {
                    if *ps != pv {
                        out.p.push((l, vec![pv, *ps]));
                    }
                    if *ns != nv {
                        out.n.push((l, vec![nv, *ns]));
                    }
                }
            }
            // And the legs that arrive at the via end at their own offsets: join.
            if let Some((pe, ne)) = ends.get(k) {
                let layer_before = centre.get(k).map(|(l, _)| *l);
                if let Some(l) = layer_before {
                    if *pe != pv {
                        out.p.push((l, vec![*pe, pv]));
                    }
                    if *ne != nv {
                        out.n.push((l, vec![*ne, nv]));
                    }
                }
            }
        }
        // Pad stubs at the start and, when the pointer is on the pair's pads, at the end.
        if let (Some((ps, ns)), Some((l, _))) = (starts.first(), centre.first()) {
            if *ps != pair.p_pad {
                out.p.push((*l, vec![pair.p_pad, *ps]));
            }
            if *ns != pair.n_pad {
                out.n.push((*l, vec![pair.n_pad, *ns]));
            }
        }
        if let (Some((tp, tn)), Some((pe, ne)), Some((l, _))) = (target, ends.last(), centre.last())
        {
            if *pe != tp {
                out.p.push((*l, vec![*pe, tp]));
            }
            if *ne != tn {
                out.n.push((*l, vec![*ne, tn]));
            }
        }
        out
    }

    /// Live numbers of the pair route (positive/negative length, difference, gap).
    pub(super) fn pair_live(&self, r: &Route, pair: &PairRoute) -> PairLive {
        let leg: Vec<Point> = self
            .route_resolved()
            .and_then(|v| v.leg.clone().ok())
            .unwrap_or_default();
        let target = self.pair_target_pads(r, pair, self.raw_cursor());
        let c = self.pair_copper(r, pair, &leg, target);
        let len = |v: &[(LayerId, Vec<Point>)]| -> f64 {
            v.iter().map(|(_, p)| diffpair::length(p)).sum()
        };
        let (lp, ln) = (len(&c.p), len(&c.n));
        let gap = c
            .coupled
            .iter()
            .filter_map(|(a, b)| diffpair::min_gap(a, b, pair.width))
            .fold(None, |m: Option<f64>, g| Some(m.map_or(g, |m| m.min(g))));
        let skew = (lp - ln).abs();
        PairLive {
            name: pair.name.clone(),
            length_p_mm: mm_str(lp.round() as i64),
            length_n_mm: mm_str(ln.round() as i64),
            skew_mm: mm_str(skew.round() as i64),
            max_skew_mm: mm_str(pair.max_skew),
            gap_mm: gap.map_or_else(String::new, |g| mm_str(g.round() as i64)),
            gap_target_mm: mm_str(pair.gap),
            over_skew: skew > pair.max_skew as f64,
        }
    }

    /// The pair as overlay shapes: both legs with their clearance halo.
    pub(super) fn pair_preview_shapes(
        &self,
        r: &Route,
        pair: &PairRoute,
        blocked: bool,
    ) -> Vec<(Shape, Rgba)> {
        let theme = self.theme_kind();
        let leg: Vec<Point> = match r.resolved.as_ref().map(|(_, v)| &v.leg) {
            Some(Ok(l)) => l.clone(),
            _ => routing::path(
                *r.pts.last().unwrap_or(&Point::ORIGIN),
                self.route_end(self.raw_cursor(), self.cursor).0,
                self.pcb_ed.route_mode,
                self.pcb_ed.route_flip,
            ),
        };
        let target = self.pair_target_pads(r, pair, self.raw_cursor());
        let c = self.pair_copper(r, pair, &leg, target);
        let clearance = self.clearance_for(r.net);
        let mut out = Vec::new();
        for (net_polys, vias) in [(&c.p, &c.p_vias), (&c.n, &c.n_vias)] {
            for (l, poly) in net_polys {
                let col = if blocked {
                    Rgba::rgb(230, 60, 60)
                } else {
                    ps::layer_color(*l, theme)
                };
                let halo = Rgba::with_alpha(col.0[0], col.0[1], col.0[2], 70);
                for w in poly.windows(2) {
                    let seg = Segment::new(w[0], w[1]);
                    out.push((
                        Shape::Line {
                            segment: seg,
                            width: pair.width + 2 * clearance,
                        },
                        halo,
                    ));
                    out.push((
                        Shape::Line {
                            segment: seg,
                            width: pair.width,
                        },
                        col,
                    ));
                }
            }
            for v in vias {
                out.push((
                    Shape::Circle {
                        circle: Circle {
                            center: *v,
                            radius: self.via_size_for(r.net).0 / 2,
                        },
                        filled: true,
                        stroke: 0,
                    },
                    Rgba::rgb(212, 175, 55),
                ));
            }
        }
        out
    }

    /// Commits the pair route: validated, both nets, one undo step.
    pub(super) fn pair_commit(
        &mut self,
        r: Route,
        pair: &PairRoute,
        backup: Route,
        out: &mut Outcome,
    ) {
        let target = self.pair_target_pads(&r, pair, self.raw_cursor());
        let c = self.pair_copper(&r, pair, &[], target);
        let (vd, vdr) = self.via_size_for(r.net);
        if self.pcb_ed.obstacles != ObstacleMode::Ignore {
            let problems = self.pcb_doc().map(|doc| {
                let mk = |net: Id, tracks: &'_ [(LayerId, Vec<Point>)], vias: &'_ [Point]| {
                    (net, tracks.to_vec(), vias.to_vec())
                };
                let (pn, pt, pv) = mk(pair.pos, &c.p, &c.p_vias);
                let (nn, nt, nv) = mk(pair.neg, &c.n, &c.n_vias);
                buturecad_router::validate_pair(
                    doc,
                    &RouteCandidate {
                        net: Some(pn),
                        width: pair.width,
                        tracks: &pt,
                        vias: &pv,
                        via_diameter: vd,
                    },
                    &RouteCandidate {
                        net: Some(nn),
                        width: pair.width,
                        tracks: &nt,
                        vias: &nv,
                        via_diameter: vd,
                    },
                )
            });
            if let Some(first) = problems.as_ref().and_then(|p| p.first()) {
                self.log(
                    MessageLevel::Warn,
                    format!("The pair would break a design rule ({first}); nothing was created. Move it, or switch to “Ignore obstacles”"),
                );
                self.pcb_ed.route = Some(backup);
                out.redraw = true;
                return;
            }
        }
        let mut changes: Vec<PcbChange> = Vec::new();
        let mut created: BTreeSet<Id> = BTreeSet::new();
        for (net, polys) in [(pair.pos, &c.p), (pair.neg, &c.n)] {
            for (l, poly) in polys {
                for w in poly.windows(2) {
                    let id = self.pcb_new_id();
                    created.insert(id);
                    changes.push(PcbChange::create(
                        id,
                        PcbObject::Track(Track {
                            layer: *l,
                            segment: Segment::new(w[0], w[1]),
                            width: pair.width,
                            net: Some(net),
                        }),
                    ));
                }
            }
        }
        for (net, vias) in [(pair.pos, &c.p_vias), (pair.neg, &c.n_vias)] {
            for at in vias {
                let id = self.pcb_new_id();
                created.insert(id);
                changes.push(PcbChange::create(
                    id,
                    PcbObject::Via(Via {
                        position: *at,
                        diameter: vd,
                        drill: vdr,
                        net: Some(net),
                        kind: ViaKind::Through,
                    }),
                ));
            }
        }
        if changes.is_empty() {
            out.redraw = true;
            return;
        }
        let live = self.pair_live(&r, pair);
        if live.over_skew {
            self.log(
                MessageLevel::Warn,
                format!(
                    "Pair {}: the legs differ by {} mm (maximum {} mm); DRC will report it",
                    pair.name, live.skew_mm, live.max_skew_mm
                ),
            );
        }
        if self.pcb_commit("Route differential pair", changes) {
            self.selection.insert(Mode::Pcb, created);
            out.model_changed = true;
            out.selection_changed = true;
        }
        out.redraw = true;
    }
}
