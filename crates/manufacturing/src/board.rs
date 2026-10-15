//! Board → [`FabInput`]: the bridge from the real PCB model to the manufacturing outputs.
//!
//! Lives next to the writers (not in the GUI) so that the desktop app, the CLI and tests build
//! the **same** production files from a board. It runs a fresh DRC, reads the copper geometry the
//! DRC itself uses (`buturecad_eda_core::copper`), and turns pads, tracks, vias, zone fills,
//! silkscreen, outline, drills and parts into plain nanometre geometry.

use crate::font;
use crate::{Aperture, Feature, Issue, Layer, PartFields, Severity};
use buturecad_eda_core::copper::{self, CShape, ItemKind};
use buturecad_eda_core::model::{Id, PcbDoc, ViaKind};
use buturecad_eda_core::outline::{arc_points, outline_loops};
use buturecad_eda_core::Project;
use buturecad_geometry::Point;
use buturecad_pcb::{layer, FpShape, LayerId, MountType, PadKind, PadShape, Placement, TextKind};
use buturecad_rules::run_drc;
use buturecad_schematic::Object;
use std::collections::BTreeMap;
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

/// Options of the export that are about presentation, not about the board.
#[derive(Debug, Clone, Default)]
pub struct BuildOptions {
    /// Also print each part's value on the silkscreen (references are always printed).
    pub silk_values: bool,
    /// Rotation corrections for the pick-and-place file: `(footprint pattern, degrees)`.
    pub rotation_offsets: Vec<(String, i32)>,
}

/// Minimum silkscreen stroke for text (nm) and the smallest legible text height (nm).
const TEXT_STROKE_MIN: i64 = 150_000;
const TEXT_HEIGHT_MIN: i64 = 800_000;

fn quarter(p: Point, n: u8) -> Point {
    p.rotate_quarter_turns(Point::ORIGIN, n % 4)
}

/// Stroked text as silkscreen features. `rotation` is in quarter turns; `place` is the footprint
/// placement the text belongs to (it flips and rotates the glyphs with the part), or `None` for
/// board text, which is mirrored when `mirror` is set (text on the bottom side).
#[allow(clippy::too_many_arguments)]
fn text_features(
    out: &mut Vec<Feature>,
    text: &str,
    centre: Point,
    height: i64,
    rotation: u8,
    place: Option<&Placement>,
    mirror: bool,
    unknown: &mut usize,
) {
    if text.is_empty() || height <= 0 {
        return;
    }
    let s = font::strokes(text, height);
    *unknown += s.unknown;
    let width = (height / 8).max(TEXT_STROKE_MIN);
    let map = |q: crate::Pt| {
        let mut p = quarter(Point::new(q.x, q.y), rotation);
        match place {
            Some(pl) => {
                if pl.flipped {
                    p = Point::new(-p.x, p.y);
                }
                p = quarter(p, pl.rotation);
            }
            None if mirror => p = Point::new(-p.x, p.y),
            None => {}
        }
        pt(Point::new(centre.x + p.x, centre.y + p.y))
    };
    for line in &s.polylines {
        for w in line.windows(2) {
            let (a, b) = (map(w[0]), map(w[1]));
            if a != b {
                out.push(Feature::Draw { a, b, width });
            }
        }
    }
}

/// The purchasing fields of every schematic component, by component id. Property names are
/// matched without regard to case (`MPN`, `mpn`, `Mpn`).
pub fn part_fields(project: &Project) -> BTreeMap<Id, PartFields> {
    let mut out = BTreeMap::new();
    for sheet in project.schematic().sheets().values() {
        for (id, o) in &sheet.objects {
            let Object::Component(c) = o else { continue };
            let get = |key: &str| {
                c.properties
                    .iter()
                    .find(|(k, _)| k.eq_ignore_ascii_case(key))
                    .map(|(_, v)| v.trim().to_owned())
                    .unwrap_or_default()
            };
            out.insert(
                *id,
                PartFields {
                    manufacturer: get("manufacturer"),
                    mpn: get("mpn"),
                    supplier: get("supplier"),
                    supplier_pn: get("spn"),
                },
            );
        }
    }
    out
}

/// Builds the manufacturing input of a whole project: the board plus the purchasing data of its
/// parts (read from the schematic components the footprints were created from).
pub fn build_fab_input_for_project(
    project: &Project,
    name: &str,
    opts: &BuildOptions,
) -> crate::FabInput {
    build_fab_input_with(project.pcb(), name, &part_fields(project), opts)
}

/// Builds the manufacturing input from the board alone (no purchasing data, references only on
/// the silkscreen). Runs DRC first (fresh, not the cached report).
pub fn build_fab_input(doc: &PcbDoc, name: &str) -> crate::FabInput {
    build_fab_input_with(doc, name, &BTreeMap::new(), &BuildOptions::default())
}

/// The general form of [`build_fab_input`].
pub fn build_fab_input_with(
    doc: &PcbDoc,
    name: &str,
    fields: &BTreeMap<Id, PartFields>,
    opts: &BuildOptions,
) -> crate::FabInput {
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
    // Silkscreen text: references and user texts (values only on request), board texts.
    let (mut unknown, mut small) = (0usize, 0usize);
    for f in doc.placed.values() {
        let Some(def) = doc.footprints.get(&f.footprint) else {
            continue;
        };
        for t in &def.texts {
            let text = match t.kind {
                TextKind::Reference => f.reference.as_str(),
                TextKind::Value if opts.silk_values => f.value.as_str(),
                TextKind::Value => continue,
                TextKind::User => t.text.as_str(),
            };
            // A value is normally on the fabrication layer; when it is requested it goes on the
            // silkscreen of the part's side.
            let target = if t.kind == TextKind::Value {
                f.placement.board_layer(layer::F_SILK)
            } else {
                f.placement
                    .board_layer(buturecad_pcb::fp_layer_front(t.layer))
            };
            let out = if target == layer::F_SILK {
                &mut silk_f
            } else if target == layer::B_SILK {
                &mut silk_b
            } else {
                continue;
            };
            if !text.is_empty() && t.height < TEXT_HEIGHT_MIN {
                small += 1;
            }
            text_features(
                out,
                text,
                f.placement.apply(t.position),
                t.height,
                t.rotation,
                Some(&f.placement),
                false,
                &mut unknown,
            );
        }
    }
    for t in doc.texts.values() {
        let out = if t.layer == layer::F_SILK {
            &mut silk_f
        } else if t.layer == layer::B_SILK {
            &mut silk_b
        } else {
            continue;
        };
        if !t.text.is_empty() && t.height < TEXT_HEIGHT_MIN {
            small += 1;
        }
        text_features(
            out,
            &t.text,
            t.position,
            t.height,
            t.rotation,
            None,
            t.layer == layer::B_SILK,
            &mut unknown,
        );
    }
    extra.push(Issue::new(
        Severity::Info,
        "silk_text",
        format!(
            "Silkscreen text is drawn with ButureCAD's stroke font ({} stroke minimum): references and user texts are exported{}; text is not checked against pads or the board edge, and filled shapes are drawn as outlines.",
            crate::fmt_mm(TEXT_STROKE_MIN),
            if opts.silk_values { ", and values" } else { ", values are not" }
        ),
    ));
    if small > 0 {
        extra.push(Issue::new(
            Severity::Warning,
            "silk_text_small",
            format!(
                "{small} silkscreen text(s) are lower than {} mm and may be illegible when printed.",
                crate::fmt_mm(TEXT_HEIGHT_MIN)
            ),
        ));
    }
    if unknown > 0 {
        extra.push(Issue::new(
            Severity::Warning,
            "silk_text_glyph",
            format!("{unknown} silkscreen character(s) have no glyph in the stroke font and are drawn as boxes."),
        ));
    }
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
            fields: f
                .link
                .and_then(|id| fields.get(&id))
                .cloned()
                .unwrap_or_default(),
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
        rotation_offsets: opts.rotation_offsets.clone(),
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
