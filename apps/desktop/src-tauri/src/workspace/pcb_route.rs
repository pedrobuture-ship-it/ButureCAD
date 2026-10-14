//! Interactive track routing and vias: the route state machine and its previews.
//!
//! A route starts on a pad, via or track (the net comes from it) or on empty copper (no net),
//! grows vertex by vertex following the 45°/90°/free mode, may change copper layer through a
//! through via (`V`), and finishes on a pad/via/track of the same net, with Enter or a double
//! click. Everything created by one route (tracks and vias) is **one** `PcbEditCommand`, so one
//! undo removes the whole route.
//!
//! **Obstacle modes:** *Walk around* (default) routes every leg around the copper, keepouts and
//! board edge of other nets with the net class clearance (`buturecad-router`), and refuses to
//! commit a route that fails the same DRC rules the DRC panel applies; *Ignore obstacles* draws
//! exactly what you click (DRC reports the consequences afterwards).

use super::pcb_editor::{mm_str, parse_mm, Drag, PcbTool};
use super::*;
use crate::pcb_scene as ps;
use buturecad_eda_core::model::{PcbChange, PcbObject, Track, Via, ViaKind};
use buturecad_eda_core::routing::{self, AngleMode};
use buturecad_geometry::{Circle, Segment};
use buturecad_pcb::{layer, LayerId};
use buturecad_renderer::{Rgba, Shape};
use buturecad_router::{NewCopper, ObstacleSet, RouteCandidate, RouteParams, ShoveResult};

/// How the interactive router treats obstacles.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub(crate) enum ObstacleMode {
    #[default]
    WalkAround,
    /// Existing tracks of other nets are pushed aside (falls back to walking around).
    Push,
    Ignore,
}

impl ObstacleMode {
    pub(crate) fn id(self) -> &'static str {
        match self {
            ObstacleMode::WalkAround => "walk",
            ObstacleMode::Push => "push",
            ObstacleMode::Ignore => "ignore",
        }
    }
    fn parse(s: &str) -> Option<Self> {
        match s {
            "walk" => Some(ObstacleMode::WalkAround),
            "push" => Some(ObstacleMode::Push),
            "ignore" => Some(ObstacleMode::Ignore),
            _ => None,
        }
    }
}

/// Obstacles built for one `(revision, params)`.
pub(crate) struct ObstacleCache {
    revision: u64,
    params: RouteParams,
    /// Built without other nets' tracks (Push & Shove).
    fixed_only: bool,
    set: ObstacleSet,
}

impl Resolved {
    pub(super) fn new(leg: Result<Vec<Point>, String>, note: Option<String>) -> Self {
        Self {
            leg,
            shove: None,
            note,
        }
    }
}

/// What a route resolved to for the current pointer position (computed once per change, then
/// read by the preview, the bar and the click handlers).
#[derive(Clone)]
pub(crate) struct Resolved {
    pub(super) leg: Result<Vec<Point>, String>,
    pub(super) shove: Option<ShoveResult>,
    pub(super) note: Option<String>,
}

#[derive(Clone, PartialEq)]
pub(crate) struct ResolveKey {
    end: Point,
    last: Option<Point>,
    pts: usize,
    done: usize,
    vias: usize,
    layer: LayerId,
    net: Option<Id>,
    revision: u64,
    flip: bool,
    angle: AngleMode,
    width: i64,
    via: (i64, i64),
    obstacles: ObstacleMode,
}

/// A route in progress.
#[derive(Clone)]
pub(crate) struct Route {
    pub(super) net: Option<Id>,
    pub(super) layer: LayerId,
    /// Vertices on the current layer since the start or the last via.
    pub(super) pts: Vec<Point>,
    /// Finished polylines on earlier layers.
    pub(super) done: Vec<(LayerId, Vec<Point>)>,
    pub(super) vias: Vec<Point>,
    /// Outlines of the net's copper, computed once per route (and after each via).
    pub(super) highlight: Vec<Shape>,
    pub(super) resolved: Option<(ResolveKey, Resolved)>,
    /// Set for a differential pair route: `net` is the positive net and `pts`/`done` the centre
    /// path of the pair.
    pub(super) pair: Option<super::pcb_pair::PairRoute>,
}

/// What is under the pointer that a route can start on or end at.
pub(super) struct Hit {
    pub(super) net: Option<Id>,
    pub(super) at: Point,
    pub(super) layers: Vec<LayerId>,
    pub(super) what: &'static str,
}

impl Workspace {
    /// The net being routed (none when idle or on netless copper).
    pub(crate) fn route_net(&self) -> Option<Id> {
        self.pcb_ed.route.as_ref().and_then(|r| r.net)
    }

    /// Track width for `net`: the manual setting, else the net class.
    pub(crate) fn route_width_for(&self, net: Option<Id>) -> i64 {
        self.pcb_ed.route_width.unwrap_or_else(|| {
            self.pcb_doc()
                .map_or(buturecad_eda_core::routing::DEFAULT_WIDTH, |d| {
                    d.rules.class_of(net).track_width
                })
        })
    }

    /// Via `(diameter, drill)` for `net`: the manual setting, else the net class.
    pub(crate) fn via_size_for(&self, net: Option<Id>) -> (i64, i64) {
        self.pcb_ed.via_size.unwrap_or_else(|| {
            self.pcb_doc().map_or((800_000, 400_000), |d| {
                let c = d.rules.class_of(net);
                (c.via_diameter, c.via_drill)
            })
        })
    }

    /// Clearance shown around a route of `net` (the class value; DRC enforces it).
    pub(crate) fn clearance_for(&self, net: Option<Id>) -> i64 {
        self.pcb_doc()
            .map_or(buturecad_eda_core::routing::DEFAULT_CLEARANCE, |d| {
                d.rules.class_of(net).clearance
            })
    }

    pub(super) fn route_params(&self, net: Option<Id>, layer: LayerId) -> RouteParams {
        RouteParams::new(net, layer, self.route_width_for(net))
    }

    /// One obstacle-aware leg (walk around). `fixed_only` leaves other nets' tracks out of the
    /// obstacles (Push & Shove moves them instead). The error is a sentence for the user.
    pub(super) fn leg_with(
        &self,
        params: RouteParams,
        from: Point,
        to: Point,
        fixed_only: bool,
    ) -> Result<Vec<Point>, String> {
        let (mode, flip) = (self.pcb_ed.route_mode, self.pcb_ed.route_flip);
        let (Some(doc), Some(open)) = (self.pcb_doc(), self.open.as_ref()) else {
            return Ok(routing::path(from, to, mode, flip));
        };
        let revision = open.engine.revision();
        let mut cache = self.pcb_ed.obstacle_cache.borrow_mut();
        let stale = cache.as_ref().is_none_or(|c| {
            c.revision != revision || c.params != params || c.fixed_only != fixed_only
        });
        if stale {
            *cache = Some(ObstacleCache {
                revision,
                params,
                fixed_only,
                set: ObstacleSet::collect_with(doc, params, !fixed_only),
            });
        }
        let Some(c) = cache.as_ref() else {
            return Ok(routing::path(from, to, mode, flip));
        };
        buturecad_router::route_leg(&c.set, from, to, mode, flip).map_err(|e| e.to_string())
    }

    /// Pushes existing tracks aside for `tracks`/`vias` of net `net`.
    fn shove_for(
        &self,
        net: Option<Id>,
        tracks: &[(LayerId, Vec<Point>)],
        vias: &[Point],
    ) -> Result<ShoveResult, String> {
        let doc = self.pcb_doc().ok_or("no project is open")?;
        buturecad_router::shove(
            doc,
            &NewCopper {
                net,
                width: self.route_width_for(net),
                tracks,
                vias,
                via_diameter: self.via_size_for(net).0,
            },
        )
        .map_err(|e| e.to_string())
    }

    fn compute_resolved(&self, r: &Route, end: Point) -> Resolved {
        if let Some(pair) = &r.pair {
            return self.pair_resolve(r, pair, end);
        }
        let last = *r.pts.last().unwrap_or(&end);
        let (mode, flip) = (self.pcb_ed.route_mode, self.pcb_ed.route_flip);
        let plain = |leg| Resolved {
            leg,
            shove: None,
            note: None,
        };
        match self.pcb_ed.obstacles {
            ObstacleMode::Ignore => plain(Ok(routing::path(last, end, mode, flip))),
            ObstacleMode::WalkAround => {
                plain(self.leg_with(self.route_params(r.net, r.layer), last, end, false))
            }
            ObstacleMode::Push => {
                let leg = match self.leg_with(self.route_params(r.net, r.layer), last, end, true) {
                    Ok(l) => l,
                    Err(e) => return plain(Err(e)),
                };
                let mut poly = r.pts.clone();
                poly.extend(leg.iter().skip(1));
                let mut tracks = r.done.clone();
                tracks.push((r.layer, poly));
                match self.shove_for(r.net, &tracks, &r.vias) {
                    Ok(res) => {
                        let note = (!res.moved.is_empty() || !res.removed.is_empty()).then(|| {
                            format!("pushing {} track(s)", res.moved.len() + res.removed.len())
                        });
                        Resolved {
                            leg: Ok(leg),
                            shove: Some(res),
                            note,
                        }
                    }
                    Err(why) => {
                        match self.leg_with(self.route_params(r.net, r.layer), last, end, false) {
                            Ok(l) => Resolved {
                                leg: Ok(l),
                                shove: None,
                                note: Some(format!("cannot push ({why}); walking around")),
                            },
                            Err(_) => plain(Err(why)),
                        }
                    }
                }
            }
        }
    }

    /// Recomputes how the route in progress continues to the pointer (cached per change) and
    /// shows pushed tracks in their new place. Called wherever the pointer or the route changes.
    pub(crate) fn route_refresh(&mut self) {
        let Some(r) = self.pcb_ed.route.as_ref() else {
            return;
        };
        let (end, _) = self.route_end(self.raw_cursor(), self.cursor);
        let key = ResolveKey {
            end,
            last: r.pts.last().copied(),
            pts: r.pts.len(),
            done: r.done.len(),
            vias: r.vias.len(),
            layer: r.layer,
            net: r.net,
            revision: self.open.as_ref().map_or(0, |o| o.engine.revision()),
            flip: self.pcb_ed.route_flip,
            angle: self.pcb_ed.route_mode,
            width: r
                .pair
                .as_ref()
                .map_or_else(|| self.route_width_for(r.net), |p| 2 * p.width + p.gap),
            via: self.via_size_for(r.net),
            obstacles: self.pcb_ed.obstacles,
        };
        if r.resolved.as_ref().is_some_and(|(k, _)| *k == key) {
            return;
        }
        let resolved = self.compute_resolved(r, end);
        self.route_show_pushed(resolved.shove.as_ref());
        if let Some(r) = self.pcb_ed.route.as_mut() {
            r.resolved = Some((key, resolved));
        }
    }

    /// Draws the tracks a push moves at their new place (the model is untouched until commit).
    fn route_show_pushed(&mut self, shove: Option<&ShoveResult>) {
        let mut now: BTreeMap<Id, Option<Segment>> = BTreeMap::new();
        if let Some(s) = shove {
            for (id, seg) in &s.moved {
                now.insert(*id, Some(*seg));
            }
            for id in &s.removed {
                now.insert(*id, None);
            }
        }
        let Some(open) = self.open.as_ref() else {
            return;
        };
        let doc = open.engine.project().pcb();
        let previous: Vec<Id> = self.pcb_ed.touched.iter().copied().collect();
        for id in previous {
            if !now.contains_key(&id) {
                let o = doc.object(id);
                self.pcb_scene.sync(doc, id, o.as_ref());
                self.pcb_ed.touched.remove(&id);
            }
        }
        for (id, seg) in now {
            let moved = seg.and_then(|seg| {
                doc.tracks.get(&id).map(|t| {
                    let mut t = t.clone();
                    t.segment = seg;
                    PcbObject::Track(t)
                })
            });
            self.pcb_scene.sync(doc, id, moved.as_ref());
            self.pcb_ed.touched.insert(id);
        }
    }

    /// The resolved continuation of the route (call [`Self::route_refresh`] first).
    pub(super) fn route_resolved(&self) -> Option<&Resolved> {
        self.pcb_ed
            .route
            .as_ref()
            .and_then(|r| r.resolved.as_ref().map(|(_, v)| v))
    }

    pub(super) fn route_tolerance(&self) -> f64 {
        self.views
            .get(&Mode::Pcb)
            .map_or(100_000.0, |v| v.px_to_nm(6.0))
    }

    pub(super) fn raw_cursor(&self) -> Point {
        self.views.get(&Mode::Pcb).map_or(self.cursor, |v| {
            v.screen_to_world(self.last_pointer.0, self.last_pointer.1)
        })
    }

    pub(super) fn pcb_hit(&self, raw: Point) -> Option<Hit> {
        let doc = self.pcb_doc()?;
        let tol = self.route_tolerance();
        let stack = &doc.stackup;
        for f in doc.placed.values() {
            let Some(def) = doc.footprints.get(&f.footprint) else {
                continue;
            };
            for p in f.world_pads(def) {
                if p.copper_layers(stack).is_empty() {
                    continue;
                }
                if p.distance_to_point(raw) <= tol {
                    return Some(Hit {
                        net: f.pad_nets.get(&p.number).copied(),
                        at: p.center,
                        layers: p.copper_layers(stack),
                        what: "pad",
                    });
                }
            }
        }
        for v in doc.vias.values() {
            if v.position.distance(raw) <= v.diameter as f64 / 2.0 + tol {
                return Some(Hit {
                    net: v.net,
                    at: v.position,
                    layers: v.kind.layers(stack),
                    what: "via",
                });
            }
        }
        for t in doc.tracks.values() {
            if t.segment.distance_to_point(raw) <= t.width as f64 / 2.0 + tol {
                let end = [t.segment.a, t.segment.b]
                    .into_iter()
                    .find(|e| e.distance(raw) <= t.width as f64 / 2.0 + 2.0 * tol);
                return Some(Hit {
                    net: t.net,
                    at: end.unwrap_or_else(|| t.segment.nearest_point(raw)),
                    layers: vec![t.layer],
                    what: "track",
                });
            }
        }
        None
    }

    pub(super) fn net_name(&self, n: Option<Id>) -> String {
        n.and_then(|id| self.pcb_doc()?.nets.get(&id))
            .map_or_else(|| "(no net)".into(), |n| n.name.clone())
    }

    pub(super) fn net_highlight(&self, net: Id) -> Vec<Shape> {
        let Some(doc) = self.pcb_doc() else {
            return Vec::new();
        };
        let mut out = Vec::new();
        for f in doc.placed.values() {
            let Some(def) = doc.footprints.get(&f.footprint) else {
                continue;
            };
            for p in f.world_pads(def) {
                if f.pad_nets.get(&p.number) == Some(&net) {
                    out.extend(ps::world_pad_shape(&p, 80_000, false));
                }
            }
        }
        for t in doc.tracks.values().filter(|t| t.net == Some(net)) {
            out.push(Shape::Line {
                segment: t.segment,
                width: t.width + 120_000,
            });
        }
        for v in doc.vias.values().filter(|v| v.net == Some(net)) {
            out.push(Shape::Circle {
                circle: Circle {
                    center: v.position,
                    radius: v.diameter / 2 + 60_000,
                },
                filled: false,
                stroke: 60_000,
            });
        }
        out
    }

    pub(super) fn active_copper(&self) -> LayerId {
        let l = LayerId(self.active_pcb_layer);
        match self.pcb_doc() {
            Some(d) if d.stackup.has_copper(l) => l,
            _ => layer::F_CU,
        }
    }

    // -- the state machine -----------------------------------------------------------------------

    pub(crate) fn pcb_route_click(&mut self, at: Point, out: &mut Outcome) {
        let raw = self.raw_cursor();
        if self.pcb_ed.route.is_none() {
            self.route_start(raw, at, out);
        } else {
            self.route_add(raw, at, out);
        }
    }

    fn route_start(&mut self, raw: Point, at: Point, out: &mut Outcome) {
        if self.pcb_ed.tool == PcbTool::RoutePair {
            self.pair_start(raw, out);
            return;
        }
        let hit = self.pcb_hit(raw);
        let active = self.active_copper();
        let (net, start, route_layer) = match &hit {
            Some(h) => {
                let l = if h.layers.contains(&active) {
                    active
                } else {
                    h.layers.first().copied().unwrap_or(active)
                };
                (h.net, h.at, l)
            }
            None => (None, at, active),
        };
        if route_layer != active {
            self.log(
                MessageLevel::Info,
                format!(
                    "The {} is on {}; routing there",
                    hit.as_ref().map_or("item", |h| h.what),
                    layer::name(route_layer)
                ),
            );
        }
        self.active_pcb_layer = route_layer.0;
        let highlight = net.map(|n| self.net_highlight(n)).unwrap_or_default();
        self.pcb_ed.route = Some(Route {
            net,
            layer: route_layer,
            pts: vec![start],
            done: Vec::new(),
            vias: Vec::new(),
            highlight,
            resolved: None,
            pair: None,
        });
        out.redraw = true;
        out.selection_changed = true; // refresh the bar: a route is in progress
    }

    /// Where the next vertex lands: on a same-net pad/via/track end under the pointer, else the
    /// snapped cursor.
    pub(super) fn route_end(&self, raw: Point, snapped: Point) -> (Point, Option<Hit>) {
        let Some(r) = &self.pcb_ed.route else {
            return (snapped, None);
        };
        if let Some(pair) = &r.pair {
            return self.pair_end(r, pair, raw, snapped);
        }
        match self.pcb_hit(raw) {
            Some(h) if r.net.is_some() && h.net == r.net && h.layers.contains(&r.layer) => {
                (h.at, Some(h))
            }
            other => (snapped, other),
        }
    }

    fn route_add(&mut self, raw: Point, snapped: Point, out: &mut Outcome) {
        self.route_refresh();
        let (end, hit) = self.route_end(raw, snapped);
        let Some(r) = self.pcb_ed.route.as_ref() else {
            return;
        };
        let last = *r.pts.last().unwrap_or(&end);
        if end == last {
            return;
        }
        // Refuse to short another net.
        if let Some(h) = &hit {
            if h.net.is_some() && r.net.is_some() && h.net != r.net {
                let msg = format!(
                    "That {} belongs to net {}, the route is on {}; finish elsewhere or press Esc",
                    h.what,
                    self.net_name(h.net),
                    self.net_name(r.net)
                );
                self.log(MessageLevel::Warn, msg);
                return;
            }
            if h.net.is_some() && r.net.is_some() && h.net == r.net && !h.layers.contains(&r.layer)
            {
                self.log(
                    MessageLevel::Warn,
                    "That pad is on another layer: press V to place a via first",
                );
                return;
            }
        }
        let path = match self.route_resolved().map(|v| v.leg.clone()) {
            Some(Ok(p)) => p,
            Some(Err(why)) => {
                self.log(MessageLevel::Warn, format!("Cannot route there: {why}"));
                return;
            }
            None => return,
        };
        let target = hit
            .as_ref()
            .is_some_and(|h| h.net.is_some() && (r.net.is_none() || h.net == r.net));
        let adopt = if r.net.is_none() {
            hit.as_ref().and_then(|h| h.net)
        } else {
            None
        };
        if let Some(r) = self.pcb_ed.route.as_mut() {
            r.pts.extend(path.into_iter().skip(1));
            if adopt.is_some() {
                r.net = adopt;
            }
        }
        out.redraw = true;
        out.selection_changed = true;
        if target {
            self.route_finish(out);
        }
    }

    /// Places a through via at the end of the route and continues on the next copper layer.
    pub(crate) fn route_via(&mut self) -> Result<(), String> {
        let raw = self.raw_cursor();
        let snapped = self.cursor;
        let doc = self.pcb_doc().ok_or("no project is open")?;
        let stack = doc.stackup;
        let (end, _) = self.route_end(raw, snapped);
        self.route_refresh();
        let path = match self.route_resolved().map(|v| v.leg.clone()) {
            Some(Ok(p)) => p,
            Some(Err(why)) => return Err(format!("Cannot place the via there: {why}")),
            None => return Err("start a route first (X, then click a pad)".into()),
        };
        let r = self
            .pcb_ed
            .route
            .as_mut()
            .ok_or("start a route first (X, then click a pad)")?;
        r.pts.extend(path.into_iter().skip(1));
        let poly = routing::simplify(&r.pts);
        if poly.len() >= 2 {
            r.done.push((r.layer, poly));
        }
        r.vias.push(end);
        r.layer = stack.next_copper(r.layer, 1);
        r.pts = vec![end];
        let new_layer = r.layer;
        let net = r.net;
        let mut hl = net.map(|n| self.net_highlight(n)).unwrap_or_default();
        if let Some(neg) = self
            .pcb_ed
            .route
            .as_ref()
            .and_then(|r| r.pair.as_ref())
            .map(|p| p.neg)
        {
            hl.extend(self.net_highlight(neg));
        }
        if let Some(r) = self.pcb_ed.route.as_mut() {
            r.highlight = hl;
        }
        self.active_pcb_layer = new_layer.0;
        self.log(
            MessageLevel::Info,
            format!("Via placed; routing on {}", layer::name(new_layer)),
        );
        Ok(())
    }

    /// Finishes the route at the pointer (Enter / double click / reaching the target).
    pub(crate) fn route_finish(&mut self, out: &mut Outcome) {
        let raw = self.raw_cursor();
        let snapped = self.cursor;
        let (end, _) = self.route_end(raw, snapped);
        self.route_refresh();
        let resolved_leg = self.route_resolved().map(|v| v.leg.clone());
        let Some(mut r) = self.pcb_ed.route.take() else {
            return;
        };
        let backup = r.clone();
        // Include the leg to the pointer only when nothing was clicked since the last vertex.
        if r.pts.len() == 1 && r.done.is_empty() && r.vias.is_empty() {
            out.redraw = true;
            return; // nothing routed
        }
        let last = *r.pts.last().unwrap_or(&end);
        if r.pts.len() == 1 && !r.done.is_empty() && end != last {
            match resolved_leg.unwrap_or_else(|| Err("nothing to route".into())) {
                Ok(p) => r.pts.extend(p.into_iter().skip(1)),
                Err(why) => {
                    self.log(MessageLevel::Warn, format!("Cannot finish there: {why}"));
                    self.pcb_ed.route = Some(backup);
                    out.redraw = true;
                    return;
                }
            }
        }
        if let Some(pair) = r.pair.clone() {
            self.pair_commit(r, &pair, backup, out);
            return;
        }
        let poly = routing::simplify(&r.pts);
        if poly.len() >= 2 {
            r.done.push((r.layer, poly));
        }
        let width = self.route_width_for(r.net);
        let (vd, vdr) = self.via_size_for(r.net);
        // Push & Shove: the committed route and the displaced tracks are verified together.
        let moves: Option<ShoveResult> = if self.pcb_ed.obstacles == ObstacleMode::Push {
            self.shove_for(r.net, &r.done, &r.vias).ok()
        } else {
            None
        };
        if self.pcb_ed.obstacles != ObstacleMode::Ignore && moves.is_none() {
            let problems = self.pcb_doc().map(|doc| {
                buturecad_router::validate_route(
                    doc,
                    &RouteCandidate {
                        net: r.net,
                        width,
                        tracks: &r.done,
                        vias: &r.vias,
                        via_diameter: vd,
                    },
                )
            });
            if let Some(first) = problems.as_ref().and_then(|p| p.first()) {
                let msg = format!(
                    "The route would break a design rule ({}); nothing was created. Move it, or switch to “Ignore obstacles”",
                    first.message
                );
                self.log(MessageLevel::Warn, msg);
                self.pcb_ed.route = Some(backup);
                out.redraw = true;
                return;
            }
        }
        let mut changes: Vec<PcbChange> = Vec::new();
        let mut created: BTreeSet<Id> = BTreeSet::new();
        if let (Some(m), Some(doc)) = (&moves, self.pcb_doc()) {
            for (id, seg) in &m.moved {
                if let Some(t) = doc.tracks.get(id) {
                    let mut moved = t.clone();
                    moved.segment = *seg;
                    changes.push(PcbChange::modify(
                        *id,
                        PcbObject::Track(t.clone()),
                        PcbObject::Track(moved),
                    ));
                }
            }
            for id in &m.removed {
                if let Some(t) = doc.tracks.get(id) {
                    changes.push(PcbChange::delete(*id, PcbObject::Track(t.clone())));
                }
            }
        }
        for (l, poly) in &r.done {
            for w in poly.windows(2) {
                let id = self.pcb_new_id();
                created.insert(id);
                changes.push(PcbChange::create(
                    id,
                    PcbObject::Track(Track {
                        layer: *l,
                        segment: buturecad_geometry::Segment::new(w[0], w[1]),
                        width,
                        net: r.net,
                    }),
                ));
            }
        }
        for at in &r.vias {
            let id = self.pcb_new_id();
            created.insert(id);
            changes.push(PcbChange::create(
                id,
                PcbObject::Via(Via {
                    position: *at,
                    diameter: vd,
                    drill: vdr,
                    net: r.net,
                    kind: ViaKind::Through,
                }),
            ));
        }
        if changes.is_empty() {
            return;
        }
        let pushed = moves
            .as_ref()
            .is_some_and(|m| !m.moved.is_empty() || !m.removed.is_empty());
        let label = match (r.vias.is_empty(), pushed) {
            (_, true) => "Route track (push and shove)",
            (true, false) => "Route track",
            (false, false) => "Route track with vias",
        };
        if self.pcb_commit(label, changes) {
            self.selection.insert(Mode::Pcb, created);
            out.model_changed = true;
            out.selection_changed = true;
        }
        out.redraw = true;
    }

    /// The Via tool: one through via at the pointer, on the net of whatever copper is under it.
    pub(crate) fn pcb_place_via(&mut self, at: Point, out: &mut Outcome) {
        let raw = self.raw_cursor();
        let hit = self.pcb_hit(raw);
        let (position, net) = match &hit {
            Some(h) => (h.at, h.net),
            None => (at, None),
        };
        let id = self.pcb_new_id();
        let via = Via {
            position,
            diameter: self.via_size_for(net).0,
            drill: self.via_size_for(net).1,
            net,
            kind: ViaKind::Through,
        };
        if self.pcb_commit(
            "Place via",
            vec![PcbChange::create(id, PcbObject::Via(via))],
        ) {
            self.selection.insert(Mode::Pcb, BTreeSet::from([id]));
            out.model_changed = true;
            out.selection_changed = true;
        }
        out.redraw = true;
    }

    pub(crate) fn route_cancel(&mut self) -> bool {
        let was = self.pcb_ed.route.take().is_some();
        if was {
            self.pcb_restore_scene();
        }
        was
    }

    pub(crate) fn route_double_click(&mut self, out: &mut Outcome) {
        if self.pcb_ed.route.is_some() {
            self.route_finish(out);
        }
    }

    pub(crate) fn route_hint(&self) -> String {
        match &self.pcb_ed.route {
            None if self.pcb_ed.tool == PcbTool::RoutePair => {
                "Route pair: click a pad of a differential pair net (define pairs in Net classes…). Esc stops.".into()
            }
            None => "Route: click a pad, via or track to start (the net follows). Esc stops.".into(),
            Some(r) => format!(
                "Routing {} on {}: click for corners, V = via, Space = other elbow, Enter or double click finishes, Esc cancels.",
                self.net_name(r.net),
                layer::name(r.layer)
            ),
        }
    }

    /// Route options and actions from the IPC layer. Names: `route_width` (mm), `route_mode`
    /// (`45`, `90`, `free`), `via_size` (`diameter/drill` in mm), `route_via`, `route_finish`.
    pub(crate) fn pcb_route_command(
        &mut self,
        name: &str,
        arg: Option<&str>,
    ) -> Result<bool, String> {
        match name {
            "route_width" => {
                if matches!(arg.map(str::trim), None | Some("") | Some("auto")) {
                    self.pcb_ed.route_width = None;
                    return Ok(true);
                }
                let w = parse_mm(arg.unwrap_or(""), "width")?;
                if !(10_000..=20_000_000).contains(&w) {
                    return Err("track width must be between 0.01 mm and 20 mm".into());
                }
                self.pcb_ed.route_width = Some(w);
                Ok(true)
            }
            "route_mode" => {
                self.pcb_ed.route_mode =
                    AngleMode::parse(arg.unwrap_or("").trim()).ok_or("mode is 45, 90 or free")?;
                Ok(true)
            }
            "via_size" => {
                if matches!(arg.map(str::trim), None | Some("") | Some("auto")) {
                    self.pcb_ed.via_size = None;
                    return Ok(true);
                }
                let (d, dr) = arg
                    .unwrap_or("")
                    .split_once('/')
                    .ok_or("via size is diameter/drill in mm")?;
                let (d, dr) = (parse_mm(d, "diameter")?, parse_mm(dr, "drill")?);
                if dr <= 0 || dr >= d {
                    return Err(
                        "the drill must be positive and smaller than the via diameter".into(),
                    );
                }
                self.pcb_ed.via_size = Some((d, dr));
                Ok(true)
            }
            "route_obstacles" => {
                self.pcb_ed.obstacles = ObstacleMode::parse(arg.unwrap_or("").trim())
                    .ok_or("obstacle mode is walk or ignore")?;
                Ok(true)
            }
            "route_via" => self.route_via().map(|()| true),
            "route_finish" => {
                let mut out = Outcome::default();
                self.route_finish(&mut out);
                Ok(true)
            }
            other => {
                if self.pcb_zone_command(other, arg)? {
                    return Ok(true);
                }
                self.pcb_tune_command(other, arg)
            }
        }
    }

    // -- previews --------------------------------------------------------------------------------

    pub(crate) fn route_preview_shapes(&self) -> Vec<(Shape, Rgba)> {
        let Some(r) = &self.pcb_ed.route else {
            return Vec::new();
        };
        let Some(doc) = self.pcb_doc() else {
            return Vec::new();
        };
        let theme = self.theme_kind();
        let c = ps::layer_color(r.layer, theme);
        let halo = Rgba::with_alpha(c.0[0], c.0[1], c.0[2], 70);
        let white = match self.theme {
            ThemeName::Dark => Rgba::rgb(255, 255, 255),
            ThemeName::Light => Rgba::rgb(20, 20, 20),
        };
        let (end, hit) = self.route_end(self.raw_cursor(), self.cursor);
        let width = self.route_width_for(r.net);
        let clearance = self.clearance_for(r.net);
        let mut out: Vec<(Shape, Rgba)> = r.highlight.iter().cloned().map(|s| (s, white)).collect();
        if let Some(pair) = &r.pair {
            let blocked = !matches!(r.resolved.as_ref().map(|(_, v)| &v.leg), Some(Ok(_)));
            out.extend(self.pair_preview_shapes(r, pair, blocked));
            return out;
        }
        let mut polylines: Vec<(LayerId, Vec<Point>)> = r.done.clone();
        let mut cur = r.pts.clone();
        let mut blocked = false;
        if let Some(last) = r.pts.last() {
            match r.resolved.as_ref().map(|(_, v)| &v.leg) {
                Some(Ok(p)) => cur.extend(p.iter().skip(1).copied()),
                Some(Err(_)) | None => {
                    // Show what a plain elbow would do, in red, so it is clear why it is refused.
                    blocked = true;
                    cur.extend(
                        routing::path(*last, end, self.pcb_ed.route_mode, self.pcb_ed.route_flip)
                            .into_iter()
                            .skip(1),
                    );
                }
            }
        }
        polylines.push((r.layer, cur));
        let last_poly = polylines.len().saturating_sub(1);
        for (n, (l, poly)) in polylines.iter().enumerate() {
            let col = if blocked && n == last_poly {
                Rgba::rgb(230, 60, 60)
            } else {
                ps::layer_color(*l, theme)
            };
            for w in poly.windows(2) {
                let seg = buturecad_geometry::Segment::new(w[0], w[1]);
                out.push((
                    Shape::Line {
                        segment: seg,
                        width: width + 2 * clearance,
                    },
                    halo,
                ));
                out.push((
                    Shape::Line {
                        segment: seg,
                        width,
                    },
                    col,
                ));
            }
        }
        for v in &r.vias {
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
        // Targets: the far ends of this net's unrouted connections.
        if let Some(net) = r.net {
            let rats: Vec<Point> = self
                .ratsnest(|e| e.net_lines(net).iter().flat_map(|l| [l.a, l.b]).collect())
                .unwrap_or_default();
            for p in rats {
                out.push((
                    Shape::Circle {
                        circle: Circle {
                            center: p,
                            radius: 900_000,
                        },
                        filled: false,
                        stroke: 80_000,
                    },
                    Rgba::rgb(80, 220, 120),
                ));
            }
        }
        if hit.is_some() {
            out.push((
                Shape::Circle {
                    circle: Circle {
                        center: end,
                        radius: 500_000,
                    },
                    filled: false,
                    stroke: 90_000,
                },
                Rgba::rgb(80, 220, 120),
            ));
        }
        let _ = doc;
        out
    }

    pub(crate) fn route_info(&self) -> Option<RouteInfo> {
        let r = self.pcb_ed.route.as_ref()?;
        let mut len: f64 = r.done.iter().map(|(_, p)| routing::length(p)).sum();
        len += routing::length(&r.pts);
        let resolved = r.resolved.as_ref().map(|(_, v)| v);
        let problem = resolved.and_then(|v| v.leg.clone().err());
        Some(RouteInfo {
            problem,
            note: resolved.and_then(|v| v.note.clone()),
            pair: r.pair.as_ref().map(|p| self.pair_live(r, p)),
            pushed: resolved
                .and_then(|v| v.shove.as_ref())
                .map_or(0, |s| (s.moved.len() + s.removed.len()) as u32),
            net: self.net_name(r.net),
            layer: layer::name(r.layer),
            length_mm: mm_str(len.round() as i64),
            vias: r.vias.len() as u32,
        })
    }

    // -- vertex editing --------------------------------------------------------------------------

    /// Starts dragging a vertex when exactly one track is selected and the pointer is on one of
    /// its ends. Ends of same-net tracks on the same layer that meet there move too.
    pub(crate) fn pcb_try_vertex_drag(&mut self, raw: Point) -> bool {
        let ids = {
            let s = self.selection.get(&Mode::Pcb);
            s.map(|s| s.iter().copied().collect::<Vec<_>>())
                .unwrap_or_default()
        };
        let [id] = ids.as_slice() else { return false };
        let Some(doc) = self.pcb_doc() else {
            return false;
        };
        let Some(t) = doc.tracks.get(id) else {
            return false;
        };
        let tol = self.route_tolerance() + t.width as f64 / 2.0;
        let Some(from) = [t.segment.a, t.segment.b]
            .into_iter()
            .find(|e| e.distance(raw) <= tol)
        else {
            return false;
        };
        let (layer, net) = (t.layer, t.net);
        let ends: Vec<(Id, u8)> = doc
            .tracks
            .iter()
            .filter(|(_, o)| o.layer == layer && o.net == net)
            .flat_map(|(i, o)| [(*i, 0u8, o.segment.a), (*i, 1u8, o.segment.b)])
            .filter(|(_, _, p)| *p == from)
            .map(|(i, e, _)| (i, e))
            .collect();
        self.pcb_ed.drag = Some(Drag::Vertex { from, ends });
        true
    }

    fn vertex_changes(&self, ends: &[(Id, u8)], to: Point) -> Vec<PcbChange> {
        let Some(doc) = self.pcb_doc() else {
            return Vec::new();
        };
        let mut by_track: BTreeMap<Id, Track> = BTreeMap::new();
        for (id, end) in ends {
            let Some(base) = by_track
                .get(id)
                .cloned()
                .or_else(|| doc.tracks.get(id).cloned())
            else {
                continue;
            };
            let mut t = base;
            if *end == 0 {
                t.segment.a = to;
            } else {
                t.segment.b = to;
            }
            by_track.insert(*id, t);
        }
        by_track
            .into_iter()
            .filter_map(|(id, t)| {
                let before = doc.object(id)?;
                Some(PcbChange::modify(id, before, PcbObject::Track(t)))
            })
            .collect()
    }

    pub(crate) fn pcb_vertex_preview(&mut self, to: Point) {
        let Some(Drag::Vertex { ends, .. }) = self.pcb_ed.drag.clone() else {
            return;
        };
        let changes = self.vertex_changes(&ends, to);
        let Some(open) = self.open.as_ref() else {
            return;
        };
        let doc = open.engine.project().pcb();
        for c in changes {
            self.pcb_scene.sync(doc, c.id, c.after.as_ref());
            self.pcb_ed.touched.insert(c.id);
        }
    }

    pub(crate) fn pcb_vertex_commit(&mut self, to: Point) -> bool {
        let Some(Drag::Vertex { from, ends }) = self.pcb_ed.drag.take() else {
            return false;
        };
        if from == to {
            return false;
        }
        let changes = self.vertex_changes(&ends, to);
        self.pcb_commit("Move track vertex", changes)
    }
}
