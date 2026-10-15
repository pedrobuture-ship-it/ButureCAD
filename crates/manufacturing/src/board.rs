//! Board → [`FabInput`]: the bridge from the real PCB model to the manufacturing outputs.
//!
//! Lives next to the writers (not in the GUI) so that the desktop app, the CLI and tests build
//! the **same** production files from a board. It runs a fresh DRC, reads the copper geometry the
//! DRC itself uses (`buturecad_eda_core::copper`), and turns pads, tracks, vias, zone fills,
//! silkscreen, outline, drills and parts into plain nanometre geometry.

use crate::{Aperture, Feature, Issue, Layer, Severity};
use buturecad_eda_core::copper::{self, CShape, ItemKind};
use buturecad_eda_core::model::{PcbDoc, ViaKind};
use buturecad_eda_core::outline::{arc_points, outline_loops};
use buturecad_geometry::Point;
use buturecad_pcb::{layer, FpShape, LayerId, MountType, PadKind, PadShape};
use buturecad_rules::run_drc;
use std::f64::consts::PI;

/// Default solder-mask opening margin around a pad (nm) when the pad does not set one.
const MASK_MARGIN: i64 = 50_000;
/// Stroke width for the board profile (nm).
const PROFILE_WIDTH: i64 = 100_000;
/// Minimum silkscreen stroke (nm).
const SILK_MIN: i64 = 100_000;

fn pt(p: Point) -> crate::Pt {
    crate::Pt { x: p.x, y: p.y }
}

fn pts(v: &[Point]) -> Vec<crate::Pt> {
    v.iter().map(|p| pt(*p)).collect()
}

fn feature_of(kind: ItemKind, shape: &CShape) -> Option<Feature> {
    match shape {
        CShape::Cap { seg, r } => {
            let r = r.round() as i64;
            if r <= 0 {
                return None;
            }
            if seg.a == seg.b {
                Some(Feature::Flash {
                    at: pt(seg.a),
                    ap: Aperture::Circle { d: 2 * r },
                })
            } else if kind == ItemKind::Track {
                Some(Feature::Draw {
                    a: pt(seg.a),
                    b: pt(seg.b),
                    width: 2 * r,
                })
            } else {
                Some(Feature::Flash {
                    at: pt(Point::new((seg.a.x + seg.b.x) / 2, (seg.a.y + seg.b.y) / 2)),
                    ap: Aperture::Obround {
                        w: (seg.a.x - seg.b.x).abs() + 2 * r,
                        h: (seg.a.y - seg.b.y).abs() + 2 * r,
                    },
                })
            }
        }
        CShape::Poly(p) => Some(Feature::Region {
            outer: pts(p.vertices()),
            holes: Vec::new(),
        }),
        CShape::Region(r) => Some(Feature::Region {
            outer: pts(r.outer.vertices()),
            holes: r.holes.iter().map(|h| pts(h.vertices())).collect(),
        }),
    }
}

fn copper_name(i: usize, id: LayerId) -> (String, String, String) {
    if id == layer::F_CU {
        ("F_Cu".into(), "gtl".into(), "Top".into())
    } else if id == layer::B_CU {
        ("B_Cu".into(), "gbl".into(), "Bot".into())
    } else {
        // Inner layers: physical position `i` (1-based after the top layer).
        (format!("In{i}_Cu"), format!("g{i}"), "Inr".into())
    }
}

/// Pad outline with a margin as a mask/paste feature; `None` for pads without that side.
fn pad_aperture(p: &buturecad_pcb::WorldPad, margin: i64) -> Option<Feature> {
    let (w, h) = (p.width + 2 * margin, p.height + 2 * margin);
    if w <= 0 || h <= 0 {
        return None;
    }
    Some(match p.shape {
        PadShape::Circle => Feature::Flash {
            at: pt(p.center),
            ap: Aperture::Circle { d: w.max(h) },
        },
        PadShape::Oval => Feature::Flash {
            at: pt(p.center),
            ap: Aperture::Obround { w, h },
        },
        PadShape::Rect => Feature::Flash {
            at: pt(p.center),
            ap: Aperture::Rect { w, h },
        },
        PadShape::RoundRect => {
            let r = i64::from(p.roundrect_ratio) * w.min(h) / 1000;
            Feature::Region {
                outer: pts(&copper::rounded_rect_points(p.center, w, h, r)),
                holes: Vec::new(),
            }
        }
        PadShape::Custom => Feature::Region {
            outer: pts(&p.outline),
            holes: Vec::new(),
        },
    })
}

fn circle_pts(c: Point, r: i64, n: usize) -> Vec<Point> {
    (0..n)
        .map(|k| {
            let a = 2.0 * PI * k as f64 / n as f64;
            Point::new(
                c.x + (r as f64 * a.cos()).round() as i64,
                c.y + (r as f64 * a.sin()).round() as i64,
            )
        })
        .collect()
}

fn silk_features(shape: &FpShape, out: &mut Vec<Feature>) {
    let mut line = |a: Point, b: Point, w: i64| {
        if a != b {
            out.push(Feature::Draw {
                a: pt(a),
                b: pt(b),
                width: w.max(SILK_MIN),
            });
        }
    };
    let mut ring = |v: &[Point], w: i64| {
        for i in 0..v.len() {
            line(v[i], v[(i + 1) % v.len()], w);
        }
    };
    match shape {
        FpShape::Line { segment, width } => line(segment.a, segment.b, *width),
        FpShape::Polygon { points, width, .. } => ring(points, *width),
        FpShape::Rect { rect, width, .. } => {
            let (a, b) = (rect.min(), rect.max());
            ring(&[a, Point::new(b.x, a.y), b, Point::new(a.x, b.y)], *width);
        }
        FpShape::Circle { circle, width, .. } => {
            ring(&circle_pts(circle.center, circle.radius, 48), *width)
        }
        FpShape::Arc { arc, width } => {
            for w in arc_points(arc).windows(2) {
                line(w[0], w[1], *width);
            }
        }
    }
}

/// Builds the manufacturing input from the board. Runs DRC first (fresh, not the cached report).
pub fn build_fab_input(doc: &PcbDoc, name: &str) -> crate::FabInput {
    let drc = run_drc(doc);
    let mut extra: Vec<Issue> = Vec::new();
    let copper_ids = doc.stackup.copper();
    let n_cu = copper_ids.len();
    let base = sanitize(name);
    let mut layers: Vec<Layer> = Vec::new();

    // Copper: zones first (hole contours use clear polarity), then tracks, vias, pads.
    let mut items = Vec::new();
    let owners = doc
        .zones
        .keys()
        .chain(doc.tracks.keys())
        .chain(doc.vias.keys())
        .chain(doc.placed.keys());
    for id in owners {
        items.extend(copper::items_of(doc, *id));
    }
    let order = |k: ItemKind| match k {
        ItemKind::Zone => 0,
        ItemKind::Track => 1,
        ItemKind::Via => 2,
        ItemKind::Pad => 3,
    };
    items.sort_by_key(|i| order(i.kind));
    for (i, id) in copper_ids.iter().enumerate() {
        let (stem, ext, side) = copper_name(i, *id);
        layers.push(Layer {
            file: format!("{base}-{stem}.{ext}"),
            function: format!("Copper,L{},{side}", i + 1),
            features: items
                .iter()
                .filter(|it| it.layer == *id)
                .filter_map(|it| feature_of(it.kind, &it.shape))
                .collect(),
        });
    }

    // Mask, paste.
    let (mut mask_f, mut mask_b, mut paste_f, mut paste_b) =
        (Vec::new(), Vec::new(), Vec::new(), Vec::new());
    let mut custom_pads = 0;
    for f in doc.placed.values() {
        let Some(def) = doc.footprints.get(&f.footprint) else {
            continue;
        };
        for (wp, dp) in f.world_pads(def).iter().zip(&def.pads) {
            let margin = dp.mask_margin.unwrap_or(MASK_MARGIN);
            if wp.shape == PadShape::Custom {
                custom_pads += 1;
            }
            if wp.kind == PadKind::NonPlated {
                // Opening just larger than the hole on both sides.
                if let Some(d) = dp.drill {
                    for m in [&mut mask_f, &mut mask_b] {
                        m.push(Feature::Flash {
                            at: pt(wp.center),
                            ap: Aperture::Circle {
                                d: d.diameter + 2 * margin,
                            },
                        });
                    }
                }
                continue;
            }
            if wp.front_copper {
                mask_f.extend(pad_aperture(wp, margin));
                if wp.kind == PadKind::Smd && dp.paste {
                    paste_f.extend(pad_aperture(wp, dp.paste_margin.unwrap_or(0)));
                }
            }
            if wp.back_copper {
                mask_b.extend(pad_aperture(wp, margin));
                if wp.kind == PadKind::Smd && dp.paste {
                    paste_b.extend(pad_aperture(wp, dp.paste_margin.unwrap_or(0)));
                }
            }
        }
    }
    if custom_pads > 0 {
        extra.push(Issue::new(
            Severity::Warning,
            "custom_pad_mask",
            format!("{custom_pads} custom-shaped pad(s) get a mask opening equal to the pad (no expansion)."),
        ));
    }

    // Silkscreen.
    let (mut silk_f, mut silk_b) = (Vec::new(), Vec::new());
    for g in doc.graphics.values() {
        if g.layer == layer::F_SILK {
            silk_features(&g.shape, &mut silk_f);
        } else if g.layer == layer::B_SILK {
            silk_features(&g.shape, &mut silk_b);
        }
    }
    for f in doc.placed.values() {
        if let Some(def) = doc.footprints.get(&f.footprint) {
            for g in f.world_graphics(def) {
                if g.layer == layer::F_SILK {
                    silk_features(&g.shape, &mut silk_f);
                } else if g.layer == layer::B_SILK {
                    silk_features(&g.shape, &mut silk_b);
                }
            }
        }
    }
    extra.push(Issue::new(
        Severity::Info,
        "silk_text",
        "Silkscreen shows graphics only: text (references, values) is not exported, and filled shapes are drawn as outlines.",
    ));
    extra.push(Issue::new(
        Severity::Info,
        "mask_default",
        format!(
            "Solder-mask margin defaults to {} mm and vias are tented; arcs on the outline are exported as polylines.",
            crate::fmt_mm(MASK_MARGIN)
        ),
    ));

    for (stem, ext, func, features) in [
        ("F_Mask", "gts", "Soldermask,Top", mask_f),
        ("B_Mask", "gbs", "Soldermask,Bot", mask_b),
        ("F_Paste", "gtp", "Paste,Top", paste_f),
        ("B_Paste", "gbp", "Paste,Bot", paste_b),
        ("F_Silkscreen", "gto", "Legend,Top", silk_f),
        ("B_Silkscreen", "gbo", "Legend,Bot", silk_b),
    ] {
        // Paste and legend files are only written when they have content; mask always (an empty
        // mask file would mean "no openings", which is a statement, so keep it explicit).
        if features.is_empty() && !stem.contains("Mask") {
            continue;
        }
        layers.push(Layer {
            file: format!("{base}-{stem}.{ext}"),
            function: func.into(),
            features,
        });
    }

    // Profile.
    let loops = outline_loops(doc);
    let mut edges = Vec::new();
    for l in &loops {
        for i in 0..l.len() {
            edges.push(Feature::Draw {
                a: pt(l[i]),
                b: pt(l[(i + 1) % l.len()]),
                width: PROFILE_WIDTH,
            });
        }
    }
    layers.push(Layer {
        file: format!("{base}-Edge_Cuts.gko"),
        function: "Profile,NP".into(),
        features: edges,
    });

    // Drills.
    let mut holes = Vec::new();
    let mut bad_vias = 0;
    for v in doc.vias.values() {
        if !matches!(v.kind, ViaKind::Through) {
            bad_vias += 1;
            continue;
        }
        holes.push(crate::Hole {
            at: pt(v.position),
            diameter: v.drill,
            slot_to: None,
            plated: true,
        });
    }
    if bad_vias > 0 {
        extra.push(Issue::new(
            Severity::Error,
            "via_kind",
            format!("{bad_vias} blind/buried/micro via(s): per-span drill files are not implemented, so they would be silently dropped."),
        ));
    }
    let mut parts = Vec::new();
    for f in doc.placed.values() {
        let Some(def) = doc.footprints.get(&f.footprint) else {
            continue;
        };
        for (wp, dp) in f.world_pads(def).iter().zip(&def.pads) {
            let Some(d) = dp.drill else { continue };
            let plated = wp.kind != PadKind::NonPlated;
            let slot_to = d.slot.filter(|s| *s > d.diameter).map(|len| {
                let half = (len - d.diameter) / 2;
                if wp.width >= wp.height {
                    (
                        Point::new(wp.center.x - half, wp.center.y),
                        Point::new(wp.center.x + half, wp.center.y),
                    )
                } else {
                    (
                        Point::new(wp.center.x, wp.center.y - half),
                        Point::new(wp.center.x, wp.center.y + half),
                    )
                }
            });
            holes.push(match slot_to {
                Some((a, b)) => crate::Hole {
                    at: pt(a),
                    diameter: d.diameter,
                    slot_to: Some(pt(b)),
                    plated,
                },
                None => crate::Hole {
                    at: pt(wp.center),
                    diameter: d.diameter,
                    slot_to: None,
                    plated,
                },
            });
        }
        parts.push(crate::Part {
            reference: f.reference.clone(),
            value: f.value.clone(),
            footprint: f
                .footprint
                .rsplit(':')
                .next()
                .unwrap_or(&f.footprint)
                .into(),
            position: pt(f.placement.position),
            rotation_deg: u16::from(f.placement.rotation % 4) * 90,
            bottom: f.placement.flipped,
            mount: match def.attributes.mount {
                MountType::Smd => crate::Mount::Smd,
                MountType::ThroughHole => crate::Mount::Through,
                MountType::Unspecified => crate::Mount::Unspecified,
            },
            in_bom: !def.attributes.exclude_from_bom,
            in_position_file: !def.attributes.exclude_from_position_files,
        });
    }
    for (id, z) in &doc.zones {
        if z.fill.is_none() {
            extra.push(Issue::new(
                Severity::Warning,
                "zone_unfilled",
                format!(
                    "Zone #{} on {} has no fill, so it exports no copper.",
                    id.0,
                    layer::name(z.layer)
                ),
            ));
        }
    }

    crate::FabInput {
        name: base,
        outline: loops.iter().map(|l| pts(l)).collect(),
        copper_layers: n_cu as u8,
        thickness_nm: doc.physical.thickness(),
        layers,
        holes,
        parts,
        drc: crate::DrcSummary {
            errors: drc.errors(),
            warnings: drc.warnings(),
        },
        capabilities: crate::Capabilities::default(),
        min_track_width: doc.tracks.values().map(|t| t.width).min(),
        extra_issues: extra,
    }
}

/// File-name-safe base name.
fn sanitize(name: &str) -> String {
    let s: String = name
        .chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() || c == '-' || c == '_' {
                c
            } else {
                '_'
            }
        })
        .collect();
    if s.is_empty() {
        "board".into()
    } else {
        s
    }
}
