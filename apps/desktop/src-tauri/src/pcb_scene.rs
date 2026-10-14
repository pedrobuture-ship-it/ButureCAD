//! Board → renderer scene. Every board object becomes shapes owned by its id
//! (`owner << 12 | n`), so picking a shape resolves to the object.
//!
//! Scene layer ids: `1000 + position in the stackup's drawing order` for board layers, and fixed
//! ids above 2000 for derived layers (vias, drills, pad numbers, pad centres, ratsnest…).

use crate::scene_builder::{shape_id, ThemeKind};
use buturecad_eda_core::model::{PcbDoc, PcbObject};
use buturecad_geometry::{Circle, Point, Polygon, Rectangle};
use buturecad_pcb::{layer, FpShape, Id, PadKind, PadShape, PlacedFootprint, WorldPad};
use buturecad_renderer::{Item, Layer, LayerId as SceneLayer, ObjectId, Rgba, Scene, Shape};
use std::collections::{BTreeMap, BTreeSet};
use std::f64::consts::PI;

pub const L_HIT: SceneLayer = SceneLayer(1);
pub const L_VIA: SceneLayer = SceneLayer(2000);
pub const L_DRILL: SceneLayer = SceneLayer(2001);
pub const L_PADNUM: SceneLayer = SceneLayer(2002);
pub const L_PADCENTER: SceneLayer = SceneLayer(2003);
pub const L_RATS: SceneLayer = SceneLayer(2004);
pub const L_KEEPOUT: SceneLayer = SceneLayer(2005);

/// Scene layer of the zone fill of a copper layer: below every board layer, drawn but inert.
pub fn fill_scene_layer(l: layer::LayerId, doc: &PcbDoc) -> SceneLayer {
    SceneLayer(scene_layer(l, doc).0 - 500)
}

/// The colour a pour is drawn in: the layer colour, subdued so tracks and pads stand out.
pub fn fill_color(l: layer::LayerId, theme: ThemeKind) -> Rgba {
    let c = layer_color(l, theme).0;
    let mix = |v: u8| -> u8 {
        if theme == ThemeKind::Dark {
            (f32::from(v) * 0.42 + 11.0 * 0.58) as u8
        } else {
            (f32::from(v) * 0.40 + 248.0 * 0.60) as u8
        }
    };
    Rgba::rgb(mix(c[0]), mix(c[1]), mix(c[2]))
}

/// Scene id of the `n`th fill triangle of zone `owner` (far from the shape ids of objects, which
/// are `owner << 12 | n`; fill triangles are not pickable so they never resolve to an owner).
fn fill_id(owner: Id, n: u64) -> ObjectId {
    ObjectId((1 << 63) | (owner.0 << 24) | (n & 0xFF_FFFF))
}

/// Scene layer of a board layer.
pub fn scene_layer(l: layer::LayerId, doc: &PcbDoc) -> SceneLayer {
    let pos = doc
        .stackup
        .all_layers()
        .iter()
        .position(|x| *x == l)
        .unwrap_or(0);
    SceneLayer(1000 + pos as u16)
}

pub fn layer_color(l: layer::LayerId, theme: ThemeKind) -> Rgba {
    let dark = theme == ThemeKind::Dark;
    let c = |d: (u8, u8, u8), li: (u8, u8, u8)| {
        let (r, g, b) = if dark { d } else { li };
        Rgba::rgb(r, g, b)
    };
    match l.0 {
        0 => c((222, 70, 70), (190, 30, 30)),
        1 => c((70, 120, 225), (30, 80, 190)),
        2..=31 => c((190, 160, 70), (130, 105, 20)),
        100 | 101 => c((160, 100, 210), (110, 60, 170)),
        102 | 103 => c((150, 150, 150), (110, 110, 110)),
        104 => c((90, 220, 230), (0, 130, 150)),
        105 => c((230, 130, 220), (150, 50, 150)),
        106 | 107 => c((140, 160, 200), (70, 90, 140)),
        108 => c((220, 90, 220), (160, 40, 160)),
        109 => c((170, 80, 170), (120, 40, 120)),
        110 => c((235, 215, 70), (150, 125, 0)),
        120..=127 => c((120, 170, 120), (50, 120, 50)),
        _ => c((200, 130, 80), (150, 80, 30)),
    }
}

/// Stadium / rounded-rectangle outline (counter-clockwise); `r` is clamped to half the shorter
/// side.
pub fn rounded_rect(c: Point, w: i64, h: i64, r: i64) -> Vec<Point> {
    let r = r.clamp(0, w.min(h) / 2);
    if r < 1 {
        let (hw, hh) = (w / 2, h / 2);
        return vec![
            Point::new(c.x - hw, c.y - hh),
            Point::new(c.x + hw, c.y - hh),
            Point::new(c.x + hw, c.y + hh),
            Point::new(c.x - hw, c.y + hh),
        ];
    }
    let (hw, hh, r) = (w as f64 / 2.0, h as f64 / 2.0, r as f64);
    let steps = 10;
    let mut pts = Vec::with_capacity(4 * (steps + 1));
    for (k, (sx, sy)) in [(1.0, -1.0), (1.0, 1.0), (-1.0, 1.0), (-1.0, -1.0)]
        .into_iter()
        .enumerate()
    {
        let (cx, cy) = (c.x as f64 + sx * (hw - r), c.y as f64 + sy * (hh - r));
        for i in 0..=steps {
            let a = -PI / 2.0 + (k as f64) * PI / 2.0 + PI / 2.0 * (i as f64) / (steps as f64);
            pts.push(Point::new(
                (cx + r * a.cos()).round() as i64,
                (cy + r * a.sin()).round() as i64,
            ));
        }
    }
    pts.dedup();
    pts
}

fn poly(pts: Vec<Point>, filled: bool, stroke: i64) -> Option<Shape> {
    Polygon::new(pts).ok().map(|polygon| Shape::Polygon {
        polygon,
        filled,
        stroke,
    })
}

/// Shape of a placed pad, grown by `grow` on every side.
pub fn world_pad_shape(p: &WorldPad, grow: i64, filled: bool) -> Option<Shape> {
    let (w, h) = (p.width + 2 * grow, p.height + 2 * grow);
    if w <= 0 || h <= 0 {
        return None;
    }
    let stroke = if filled { 0 } else { 50_000 };
    match p.shape {
        PadShape::Circle => Some(Shape::Circle {
            circle: Circle {
                center: p.center,
                radius: w / 2,
            },
            filled,
            stroke,
        }),
        PadShape::Rect => Rectangle::from_center(p.center, w, h)
            .ok()
            .map(|rect| Shape::Rect {
                rect,
                filled,
                stroke,
            }),
        PadShape::Oval => poly(rounded_rect(p.center, w, h, w.min(h) / 2), filled, stroke),
        PadShape::RoundRect => {
            let r = i64::from(p.roundrect_ratio) * p.width.min(p.height) / 1000 + grow;
            poly(rounded_rect(p.center, w, h, r), filled, stroke)
        }
        PadShape::Custom => poly(p.outline.clone(), filled, stroke),
    }
}

fn text_w(s: &str, h: i64) -> i64 {
    s.chars().count() as i64 * 6 * (h / 7)
}

/// All shapes of one object with their scene layers.
pub fn object_shapes(doc: &PcbDoc, obj: &PcbObject) -> Vec<(SceneLayer, Shape)> {
    let sl = |l| scene_layer(l, doc);
    let mut out: Vec<(SceneLayer, Shape)> = Vec::new();
    match obj {
        PcbObject::Track(t) => out.push((
            sl(t.layer),
            Shape::Line {
                segment: t.segment,
                width: t.width,
            },
        )),
        PcbObject::Via(v) => {
            out.push((
                L_VIA,
                Shape::Circle {
                    circle: Circle {
                        center: v.position,
                        radius: v.diameter / 2,
                    },
                    filled: true,
                    stroke: 0,
                },
            ));
            out.push((
                L_DRILL,
                Shape::Circle {
                    circle: Circle {
                        center: v.position,
                        radius: v.drill / 2,
                    },
                    filled: true,
                    stroke: 0,
                },
            ));
        }
        PcbObject::Footprint(f) => {
            if let Some(def) = doc.footprints.get(&f.footprint) {
                footprint_shapes(doc, f, def, &mut out);
            }
        }
        PcbObject::Zone(z) => {
            out.extend(poly(z.outline.clone(), false, 100_000).map(|s| (sl(z.layer), s)))
        }
        PcbObject::Keepout(k) => {
            out.extend(poly(k.outline.clone(), false, 100_000).map(|s| (L_KEEPOUT, s)))
        }
        PcbObject::Text(t) => out.push((
            sl(t.layer),
            Shape::Text {
                origin: t.position,
                height: t.height,
                text: t.text.clone(),
            },
        )),
        PcbObject::Graphic(g) => {
            let layer = sl(g.layer);
            match &g.shape {
                FpShape::Line { segment, width } => out.push((
                    layer,
                    Shape::Line {
                        segment: *segment,
                        width: *width,
                    },
                )),
                FpShape::Rect {
                    rect,
                    filled,
                    width,
                } => out.push((
                    layer,
                    Shape::Rect {
                        rect: *rect,
                        filled: *filled,
                        stroke: *width,
                    },
                )),
                FpShape::Circle {
                    circle,
                    filled,
                    width,
                } => out.push((
                    layer,
                    Shape::Circle {
                        circle: *circle,
                        filled: *filled,
                        stroke: *width,
                    },
                )),
                FpShape::Arc { arc, width } => out.push((
                    layer,
                    Shape::Arc {
                        arc: *arc,
                        width: *width,
                    },
                )),
                FpShape::Polygon {
                    points,
                    filled,
                    width,
                } => out.extend(poly(points.clone(), *filled, *width).map(|s| (layer, s))),
            }
        }
        PcbObject::Net(_) => {}
    }
    out
}

fn footprint_shapes(
    doc: &PcbDoc,
    f: &PlacedFootprint,
    def: &buturecad_pcb::Footprint,
    out: &mut Vec<(SceneLayer, Shape)>,
) {
    let sl = |l| scene_layer(l, doc);
    for p in f.world_pads(def) {
        if p.kind != PadKind::NonPlated {
            for l in p.copper_layers(&doc.stackup) {
                if let Some(s) = world_pad_shape(&p, 0, true) {
                    out.push((sl(l), s));
                }
            }
        }
        let (mask, paste) = if f.placement.flipped {
            (layer::B_MASK, layer::B_PASTE)
        } else {
            (layer::F_MASK, layer::F_PASTE)
        };
        let src = def
            .pads
            .iter()
            .find(|d| d.number == p.number && f.placement.apply(d.position) == p.center);
        if let Some(s) = world_pad_shape(&p, src.and_then(|d| d.mask_margin).unwrap_or(0), false) {
            out.push((sl(mask), s));
        }
        if p.kind == PadKind::Smd && src.is_none_or(|d| d.paste) {
            if let Some(s) =
                world_pad_shape(&p, src.and_then(|d| d.paste_margin).unwrap_or(0), false)
            {
                out.push((sl(paste), s));
            }
        }
        if let Some(d) = src.and_then(|d| d.drill) {
            out.push((
                L_DRILL,
                Shape::Circle {
                    circle: Circle {
                        center: p.center,
                        radius: d.diameter / 2,
                    },
                    filled: true,
                    stroke: 0,
                },
            ));
        }
        if !p.number.is_empty() {
            let th = (p.width.min(p.height) / 2).clamp(300_000, 900_000);
            let tw = text_w(&p.number, th);
            out.push((
                L_PADNUM,
                Shape::Text {
                    origin: Point::new(p.center.x - tw / 2, p.center.y - th / 2),
                    height: th,
                    text: p.number.clone(),
                },
            ));
        }
        // A tiny dot at the pad centre: the snap target for routing.
        out.push((
            L_PADCENTER,
            Shape::Circle {
                circle: Circle {
                    center: p.center,
                    radius: 30_000,
                },
                filled: true,
                stroke: 0,
            },
        ));
    }
    for g in f.world_graphics(def) {
        out.extend(object_shapes(doc, &PcbObject::Graphic(g)));
    }
    for t in &def.texts {
        let text = match t.kind {
            buturecad_pcb::TextKind::Reference => f.reference.clone(),
            buturecad_pcb::TextKind::Value => f.value.clone(),
            buturecad_pcb::TextKind::User => t.text.clone(),
        };
        if text.is_empty() {
            continue;
        }
        let at = f.placement.apply(t.position);
        let w = text_w(&text, t.height);
        let layer = f
            .placement
            .board_layer(buturecad_pcb::fp_layer_front(t.layer));
        out.push((
            sl(layer),
            Shape::Text {
                origin: Point::new(at.x - w / 2, at.y - t.height / 2),
                height: t.height,
                text,
            },
        ));
    }
    if let Some((lo, hi)) = f.bounds(def) {
        if let Ok(r) = Rectangle::new(lo, hi) {
            out.push((
                L_HIT,
                Shape::Rect {
                    rect: r,
                    filled: true,
                    stroke: 0,
                },
            ));
        }
    }
}

/// The scene of a board plus an owner → shapes index.
#[derive(Default)]
pub struct PcbScene {
    pub scene: Scene,
    owners: BTreeMap<Id, Vec<ObjectId>>,
    /// Zone fill triangles per zone (kept apart from `owners`: they are not selectable parts).
    fills: BTreeMap<Id, Vec<ObjectId>>,
    /// Triangulation of each zone's fill, reused while its signature is unchanged.
    triangles: BTreeMap<Id, (u64, Vec<[Point; 3]>)>,
}

impl PcbScene {
    /// Rebuilds everything (also applies a new theme or layer visibility).
    pub fn rebuild(&mut self, doc: &PcbDoc, theme: ThemeKind, hidden: &BTreeSet<u16>) {
        self.scene = Scene::new();
        self.owners.clear();
        self.fills.clear();
        self.triangles.clear();
        let dark = theme == ThemeKind::Dark;
        self.scene.set_layer(
            L_HIT,
            Layer::new("Hit areas", Rgba::rgb(0, 0, 0)).hit_only(),
        );
        for l in doc.stackup.all_layers() {
            let mut layer = Layer::new(layer::name(l), layer_color(l, theme))
                .with_visible(!hidden.contains(&l.0));
            if matches!(
                layer::kind(l),
                Some(
                    layer::LayerKind::Mask | layer::LayerKind::Paste | layer::LayerKind::Courtyard
                )
            ) {
                layer = layer.not_snappable();
            }
            self.scene.set_layer(scene_layer(l, doc), layer);
            if layer::is_copper(l) {
                self.scene.set_layer(
                    fill_scene_layer(l, doc),
                    Layer::new(format!("{} fill", layer::name(l)), fill_color(l, theme))
                        .with_visible(!hidden.contains(&l.0))
                        .draw_only(),
                );
            }
        }
        let gold = if dark {
            Rgba::rgb(212, 175, 55)
        } else {
            Rgba::rgb(160, 120, 10)
        };
        let hole = if dark {
            Rgba::rgb(11, 14, 20)
        } else {
            Rgba::rgb(248, 249, 251)
        };
        let any_copper = doc.stackup.copper().iter().any(|l| !hidden.contains(&l.0));
        self.scene
            .set_layer(L_VIA, Layer::new("Vias", gold).with_visible(any_copper));
        self.scene
            .set_layer(L_DRILL, Layer::new("Drills", hole).not_snappable());
        self.scene.set_layer(
            L_PADNUM,
            Layer::new(
                "Pad numbers",
                if dark {
                    Rgba::rgb(250, 235, 235)
                } else {
                    Rgba::rgb(60, 10, 10)
                },
            )
            .not_snappable(),
        );
        self.scene.set_layer(
            L_PADCENTER,
            Layer::new(
                "Pad centres",
                if dark {
                    Rgba::rgb(255, 255, 255)
                } else {
                    Rgba::rgb(0, 0, 0)
                },
            ),
        );
        self.scene.set_layer(
            L_RATS,
            Layer::new(
                "Ratsnest",
                if dark {
                    Rgba::rgb(240, 240, 120)
                } else {
                    Rgba::rgb(150, 120, 0)
                },
            )
            .not_snappable(),
        );
        self.scene.set_layer(
            L_KEEPOUT,
            Layer::new(
                "Keepouts",
                if dark {
                    Rgba::rgb(255, 120, 60)
                } else {
                    Rgba::rgb(200, 70, 10)
                },
            )
            .not_snappable(),
        );
        for id in doc.ids().collect::<Vec<_>>() {
            if let Some(o) = doc.object(id) {
                self.sync(doc, id, Some(&o));
            }
        }
    }

    /// Replaces (or removes) the shapes of one object; `obj` need not be the stored state
    /// (live previews).
    pub fn sync(&mut self, doc: &PcbDoc, id: Id, obj: Option<&PcbObject>) {
        for old in self.owners.remove(&id).unwrap_or_default() {
            self.scene.remove(old);
        }
        for old in self.fills.remove(&id).unwrap_or_default() {
            self.scene.remove(old);
        }
        let Some(obj) = obj else { return };
        if let PcbObject::Zone(z) = obj {
            if let Some(fill) = &z.fill {
                let layer = fill_scene_layer(z.layer, doc);
                let mut ids = Vec::new();
                let cached = self
                    .triangles
                    .get(&id)
                    .filter(|(sig, _)| *sig == fill.signature)
                    .map(|(_, t)| t.clone());
                let tris = cached.unwrap_or_else(|| {
                    let t = buturecad_eda_core::zonefill::triangulate(&fill.regions);
                    self.triangles.insert(id, (fill.signature, t.clone()));
                    t
                });
                for (n, tri) in tris.into_iter().enumerate() {
                    if let Ok(polygon) = Polygon::new(tri.to_vec()) {
                        let sid = fill_id(id, n as u64);
                        self.scene.insert(
                            sid,
                            Item {
                                layer,
                                shape: Shape::Polygon {
                                    polygon,
                                    filled: true,
                                    stroke: 0,
                                },
                                color: None,
                            },
                        );
                        ids.push(sid);
                    }
                }
                self.fills.insert(id, ids);
            }
        }
        let mut ids = Vec::new();
        for (n, (layer, shape)) in object_shapes(doc, obj).into_iter().enumerate() {
            let sid = shape_id(id, n as u64);
            self.scene.insert(
                sid,
                Item {
                    layer,
                    shape,
                    color: None,
                },
            );
            ids.push(sid);
        }
        self.owners.insert(id, ids);
    }

    pub fn shapes_of(&self, owner: Id) -> &[ObjectId] {
        self.owners.get(&owner).map_or(&[], Vec::as_slice)
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;
    use buturecad_eda_core::model::{Track, Via};
    use buturecad_geometry::Segment;
    use buturecad_pcb::generators::Soic;
    use buturecad_pcb::Placement;

    #[test]
    fn scene_has_tracks_vias_and_footprints_on_real_layers() {
        let mut pcb = PcbDoc::default();
        pcb.tracks.insert(
            Id(1),
            Track {
                layer: layer::F_CU,
                segment: Segment::new(Point::ORIGIN, Point::new(1_000_000, 0)),
                width: 250_000,
                net: None,
            },
        );
        pcb.vias.insert(
            Id(2),
            Via {
                position: Point::new(1_000_000, 0),
                diameter: 600_000,
                drill: 300_000,
                net: None,
                kind: Default::default(),
            },
        );
        let def = Soic::narrow(8).generate().unwrap();
        let name = def.name.clone();
        pcb.footprints.insert(name.clone(), def);
        pcb.placed.insert(
            Id(3),
            PlacedFootprint {
                footprint: name,
                reference: "U1".into(),
                value: "x".into(),
                placement: Placement::at(Point::new(10_000_000, 0)),
                locked: false,
                link: None,
                pad_nets: Default::default(),
            },
        );
        let mut s = PcbScene::default();
        s.rebuild(&pcb, ThemeKind::Dark, &Default::default());
        assert_eq!(s.shapes_of(Id(1)).len(), 1);
        assert_eq!(s.shapes_of(Id(2)).len(), 2, "annulus and drill");
        assert!(
            s.shapes_of(Id(3)).len() > 30,
            "pads, mask, paste, numbers, silk, texts, hit area"
        );
        // Hiding F.Cu hides the track and the front pads, not the back copper layer.
        s.rebuild(&pcb, ThemeKind::Dark, &[0u16].into_iter().collect());
        assert!(!s.scene.is_visible(scene_layer(layer::F_CU, &pcb)));
        assert!(s.scene.is_visible(scene_layer(layer::B_CU, &pcb)));
        // A 4-layer board has inner layers in the scene.
        let mut four = pcb.clone();
        four.stackup.copper_layers = 4;
        s.rebuild(&four, ThemeKind::Dark, &Default::default());
        assert!(s
            .scene
            .layer(scene_layer(layer::inner_cu(1), &four))
            .is_some());
    }

    #[test]
    fn rounded_shapes_have_the_right_extent() {
        let pts = rounded_rect(Point::ORIGIN, 2_000_000, 1_000_000, 500_000);
        assert!(pts
            .iter()
            .all(|p| p.x.abs() <= 1_000_000 && p.y.abs() <= 500_000));
        assert_eq!(pts.iter().map(|p| p.x).max(), Some(1_000_000));
        assert_eq!(rounded_rect(Point::ORIGIN, 4, 2, 0).len(), 4);
    }
}
