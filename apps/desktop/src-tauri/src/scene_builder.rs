//! Model → renderer scene. Rendering concerns live here; the model crates know nothing of pixels.
//!
//! Every model object becomes one or more renderer shapes. Shape ids encode the owner
//! (`owner << 12 | n`), so picking a shape resolves to the model object that owns it.

use buturecad_eda_core::model::{LayerId as PcbLayer, PcbDoc};
use buturecad_geometry::{Arc, Circle, Point, Rectangle, Segment, Vector};
use buturecad_renderer::{Item, Layer, LayerId, ObjectId, Rgba, Scene, Shape};
use buturecad_schematic::{Direction, Graphic, Id, Object, Placement, Schematic, Symbol};
use std::collections::BTreeMap;

pub const SHAPE_BITS: u32 = 12;

pub fn shape_id(owner: Id, n: u64) -> ObjectId {
    ObjectId(owner.0 << SHAPE_BITS | n.min((1 << SHAPE_BITS) - 1))
}

pub fn owner_of(shape: ObjectId) -> Id {
    Id(shape.0 >> SHAPE_BITS)
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ThemeKind {
    Dark,
    Light,
}

// Schematic layers, bottom to top. Ids only order drawing.
pub const L_HIT: LayerId = LayerId(1);
pub const L_SHEET: LayerId = LayerId(5);
pub const L_BODY: LayerId = LayerId(10);
pub const L_PIN: LayerId = LayerId(12);
pub const L_BUS: LayerId = LayerId(14);
pub const L_WIRE: LayerId = LayerId(16);
pub const L_JUNCTION: LayerId = LayerId(18);
pub const L_TEXT: LayerId = LayerId(20);
pub const L_LABEL: LayerId = LayerId(22);
pub const L_NOCONNECT: LayerId = LayerId(24);

const WIRE_W: i64 = 150_000;
const BUS_W: i64 = 300_000;
const LINE_W: i64 = 150_000;
const TEXT_H: i64 = 1_270_000;

pub fn schematic_layers(theme: ThemeKind) -> Vec<(LayerId, Layer)> {
    let c = |dark: (u8, u8, u8), light: (u8, u8, u8)| {
        let (r, g, b) = if theme == ThemeKind::Dark {
            dark
        } else {
            light
        };
        Rgba::rgb(r, g, b)
    };
    vec![
        (
            L_HIT,
            Layer::new("Hit areas", Rgba::rgb(0, 0, 0)).hit_only(),
        ),
        (
            L_SHEET,
            Layer::new("Sheets", c((176, 96, 208), (130, 50, 170))),
        ),
        (
            L_BODY,
            Layer::new("Symbol bodies", c((232, 190, 90), (150, 105, 10))).not_snappable(),
        ),
        (L_PIN, Layer::new("Pins", c((226, 104, 104), (178, 40, 40)))),
        (L_BUS, Layer::new("Buses", c((70, 140, 255), (20, 80, 200)))),
        (
            L_WIRE,
            Layer::new("Wires", c((56, 204, 100), (10, 140, 50))),
        ),
        (
            L_JUNCTION,
            Layer::new("Junctions", c((56, 204, 100), (10, 140, 50))),
        ),
        (
            L_TEXT,
            Layer::new("Text", c((150, 205, 255), (30, 90, 160))).not_snappable(),
        ),
        (
            L_LABEL,
            Layer::new("Labels", c((90, 225, 225), (0, 130, 140))),
        ),
        (
            L_NOCONNECT,
            Layer::new("No-connects", c((70, 140, 255), (20, 80, 200))),
        ),
    ]
}

fn line(a: Point, b: Point, width: i64) -> Shape {
    Shape::Line {
        segment: Segment::new(a, b),
        width,
    }
}

/// Shapes of one symbol graphic, in sheet coordinates.
pub fn graphic_shapes(g: &Graphic, placement: &Placement) -> Vec<(LayerId, Shape)> {
    let mut out = Vec::new();
    match g {
        Graphic::Line { segment, width } => out.push((
            L_BODY,
            line(
                placement.apply(segment.a),
                placement.apply(segment.b),
                *width,
            ),
        )),
        Graphic::Rect {
            rect,
            filled,
            width,
        } => {
            if let Ok(r) = Rectangle::new(placement.apply(rect.min()), placement.apply(rect.max()))
            {
                out.push((
                    L_BODY,
                    Shape::Rect {
                        rect: r,
                        filled: *filled,
                        stroke: *width,
                    },
                ));
            }
        }
        Graphic::Circle {
            circle,
            filled,
            width,
        } => {
            out.push((
                L_BODY,
                Shape::Circle {
                    circle: Circle {
                        center: placement.apply(circle.center),
                        radius: circle.radius,
                    },
                    filled: *filled,
                    stroke: *width,
                },
            ));
        }
        Graphic::Arc { arc, width } => {
            // A mirror reverses the sweep direction; quarter turns do not.
            let ccw = arc.ccw != placement.mirror;
            if let Ok(a) = Arc::new(
                placement.apply(arc.center),
                placement.apply(arc.start),
                placement.apply(arc.end),
                ccw,
            ) {
                out.push((
                    L_BODY,
                    Shape::Arc {
                        arc: a,
                        width: *width,
                    },
                ));
            }
        }
        Graphic::Text {
            origin,
            height,
            text,
        } => {
            out.push((
                L_TEXT,
                Shape::Text {
                    origin: placement.apply(*origin),
                    height: *height,
                    text: text.clone(),
                },
            ));
        }
    }
    out
}

/// The pin line (or a point mark for zero-length pins), in sheet coordinates.
pub fn pin_shape(pin: &buturecad_schematic::Pin, placement: &Placement) -> (LayerId, Shape) {
    let at = placement.apply(pin.position);
    if pin.length > 0 {
        let dir = placement.apply_direction(pin.orientation).vector();
        (
            L_PIN,
            line(
                at,
                at + Vector::new(dir.x * pin.length, dir.y * pin.length),
                LINE_W,
            ),
        )
    } else {
        // Zero-length pins (power ports) still need a visible, snappable connection mark.
        (L_PIN, line(at, at, 0))
    }
}

/// Shapes of a symbol instance (one unit in one representation), in sheet coordinates.
pub fn symbol_shapes(
    sym: &Symbol,
    placement: &Placement,
    unit: u8,
    representation: u8,
) -> Vec<(LayerId, Shape)> {
    let mut out = Vec::new();
    for g in sym.graphics_for(unit, representation) {
        out.extend(graphic_shapes(&g.graphic, placement));
    }
    for pin in sym.pins_for(unit, representation) {
        out.push(pin_shape(pin, placement));
    }
    out
}

fn label_shapes(at: Point, name: &str, global: bool, dir: Direction) -> Vec<(LayerId, Shape)> {
    let w = name.chars().count() as i64 * 6 * (TEXT_H / 7);
    let (ox, oy) = match dir {
        Direction::Left => (-w - 300_000, 200_000),
        _ => (300_000, 200_000),
    };
    let mut out = vec![
        (
            L_LABEL,
            Shape::Text {
                origin: at + Vector::new(ox, oy),
                height: TEXT_H,
                text: name.to_owned(),
            },
        ),
        // Anchor marker so the connection point can be seen and snapped to.
        (
            L_LABEL,
            line(
                at,
                at + Vector::new(
                    if dir == Direction::Left {
                        -250_000
                    } else {
                        250_000
                    },
                    0,
                ),
                LINE_W,
            ),
        ),
    ];
    if global {
        if let Ok(r) = Rectangle::new(
            at + Vector::new(ox - 150_000, 0),
            at + Vector::new(ox + w + 150_000, oy + TEXT_H + 150_000),
        ) {
            out.push((
                L_LABEL,
                Shape::Rect {
                    rect: r,
                    filled: false,
                    stroke: LINE_W,
                },
            ));
        }
    }
    out
}

/// All shapes of one model object (visible shapes plus an invisible click target).
pub fn object_shapes(sch: &Schematic, obj: &Object) -> Vec<(LayerId, Shape)> {
    let mut out: Vec<(LayerId, Shape)> = Vec::new();
    match obj {
        Object::Wire(w) => out.push((
            L_WIRE,
            Shape::Line {
                segment: w.segment,
                width: WIRE_W,
            },
        )),
        Object::Bus(b) => out.push((
            L_BUS,
            Shape::Line {
                segment: b.segment,
                width: BUS_W,
            },
        )),
        Object::Junction(j) => out.push((
            L_JUNCTION,
            Shape::Circle {
                circle: Circle {
                    center: j.position,
                    radius: 450_000,
                },
                filled: true,
                stroke: 0,
            },
        )),
        Object::NoConnect(n) => {
            let d = 600_000;
            let p = n.position;
            out.push((
                L_NOCONNECT,
                line(p - Vector::new(d, d), p + Vector::new(d, d), LINE_W),
            ));
            out.push((
                L_NOCONNECT,
                line(p - Vector::new(d, -d), p + Vector::new(d, -d), LINE_W),
            ));
        }
        Object::NetLabel(l) => out.extend(label_shapes(l.position, &l.name, false, l.orientation)),
        Object::GlobalLabel(l) => {
            out.extend(label_shapes(l.position, &l.name, true, l.orientation))
        }
        Object::HierLabel(l) => out.extend(label_shapes(l.position, &l.name, true, l.orientation)),
        Object::Component(c) => {
            if let Some(sym) = sch.symbol(&c.lib_id) {
                out.extend(symbol_shapes(sym, &c.placement, c.unit, c.representation));
            }
            // Virtual parts (`#FLG1`, power flags) carry no printed reference or value.
            if !c.reference.starts_with('#') {
                // Reference and value sit to the right of the symbol, clear of its pins and wires.
                let bb = sch.object_bounds(obj);
                let x = bb.max.x + 600_000;
                let cy = bb.center().y;
                out.push((
                    L_TEXT,
                    Shape::Text {
                        origin: Point::new(x, cy + 300_000),
                        height: TEXT_H,
                        text: c.reference.clone(),
                    },
                ));
                if !c.value.is_empty() {
                    out.push((
                        L_TEXT,
                        Shape::Text {
                            origin: Point::new(x, cy - TEXT_H - 300_000),
                            height: TEXT_H,
                            text: c.value.clone(),
                        },
                    ));
                }
            }
        }
        Object::PowerSymbol(p) => {
            if let Some(sym) = sch.symbol(&p.lib_id) {
                out.extend(symbol_shapes(sym, &p.placement, 1, 1));
            }
        }
        Object::Sheet(s) => {
            out.push((
                L_SHEET,
                Shape::Rect {
                    rect: s.rect,
                    filled: false,
                    stroke: 200_000,
                },
            ));
            out.push((
                L_SHEET,
                Shape::Text {
                    origin: Point::new(s.rect.min().x, s.rect.max().y + 400_000),
                    height: TEXT_H,
                    text: s.name.clone(),
                },
            ));
            for pin in &s.pins {
                if let Ok(r) = Rectangle::from_center(pin.position, 800_000, 800_000) {
                    out.push((
                        L_SHEET,
                        Shape::Rect {
                            rect: r,
                            filled: true,
                            stroke: 0,
                        },
                    ));
                }
                out.push((
                    L_TEXT,
                    Shape::Text {
                        origin: pin.position + Vector::new(500_000, -TEXT_H / 2),
                        height: TEXT_H,
                        text: pin.name.clone(),
                    },
                ));
            }
        }
    }
    // Filled click targets for objects with an interior (thin outlines are hard to hit).
    if matches!(
        obj,
        Object::Component(_)
            | Object::PowerSymbol(_)
            | Object::NetLabel(_)
            | Object::GlobalLabel(_)
            | Object::HierLabel(_)
            | Object::Sheet(_)
    ) {
        let bb = sch.object_bounds(obj);
        if let Ok(r) = Rectangle::new(bb.min, bb.max) {
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
    out
}

/// A scene mirroring a schematic, with an owner → shapes index.
#[derive(Default)]
pub struct SchematicScene {
    pub scene: Scene,
    owners: BTreeMap<Id, Vec<ObjectId>>,
}

impl SchematicScene {
    /// Rebuilds everything from the model (also applies a new theme).
    pub fn rebuild(&mut self, sch: &Schematic, theme: ThemeKind) {
        self.scene = Scene::new();
        self.owners.clear();
        for (id, layer) in schematic_layers(theme) {
            self.scene.set_layer(id, layer);
        }
        for sheet in std::iter::once(sch.root()).filter_map(|r| sch.sheet(r)) {
            for (id, obj) in &sheet.objects {
                self.sync(sch, *id, Some(obj));
            }
        }
    }

    /// Replaces (or removes, with `None`) the shapes of one object. Used for live previews:
    /// `obj` need not be the stored state.
    pub fn sync(&mut self, sch: &Schematic, id: Id, obj: Option<&Object>) {
        for old in self.owners.remove(&id).unwrap_or_default() {
            self.scene.remove(old);
        }
        let Some(obj) = obj else { return };
        let mut ids = Vec::new();
        for (n, (layer, shape)) in object_shapes(sch, obj).into_iter().enumerate() {
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

// ---------------------------------------------------------------------------------------------
// PCB (read-only view of the skeleton PCB document)
// ---------------------------------------------------------------------------------------------

pub const PCB_VIA_LAYER: LayerId = LayerId(200);
pub const PCB_DRILL_LAYER: LayerId = LayerId(201);

/// Scene layer for a PCB copper layer.
pub fn pcb_layer_id(layer: PcbLayer) -> LayerId {
    LayerId(100 + layer.0)
}

pub fn pcb_layer_name(layer: PcbLayer) -> String {
    match layer.0 {
        0 => "F.Cu".into(),
        1 => "B.Cu".into(),
        n => format!("In{}.Cu", n - 1),
    }
}

fn pcb_layer_color(layer: PcbLayer, theme: ThemeKind) -> Rgba {
    let dark = theme == ThemeKind::Dark;
    match (layer.0, dark) {
        (0, true) => Rgba::rgb(222, 70, 70),
        (0, false) => Rgba::rgb(190, 30, 30),
        (1, true) => Rgba::rgb(70, 120, 225),
        (1, false) => Rgba::rgb(30, 80, 190),
        (_, true) => Rgba::rgb(190, 160, 70),
        (_, false) => Rgba::rgb(130, 105, 20),
    }
}

/// Copper layers shown for a PCB: F.Cu, B.Cu and any other layer carrying tracks.
pub fn pcb_copper_layers(pcb: &PcbDoc) -> Vec<PcbLayer> {
    let mut v: Vec<PcbLayer> = vec![PcbLayer(0), PcbLayer(1)];
    for t in pcb.tracks.values() {
        if !v.contains(&t.layer) {
            v.push(t.layer);
        }
    }
    v.sort_by_key(|l| l.0);
    v
}

#[derive(Default)]
pub struct PcbScene {
    pub scene: Scene,
}

impl PcbScene {
    /// Rebuilds from the PCB document, keeping the given layer visibility.
    pub fn rebuild(
        &mut self,
        pcb: &PcbDoc,
        theme: ThemeKind,
        hidden: &std::collections::BTreeSet<u16>,
    ) {
        self.scene = Scene::new();
        for l in pcb_copper_layers(pcb) {
            self.scene.set_layer(
                pcb_layer_id(l),
                Layer::new(pcb_layer_name(l), pcb_layer_color(l, theme))
                    .with_visible(!hidden.contains(&l.0)),
            );
        }
        let gold = if theme == ThemeKind::Dark {
            Rgba::rgb(212, 175, 55)
        } else {
            Rgba::rgb(160, 120, 10)
        };
        let hole = if theme == ThemeKind::Dark {
            Rgba::rgb(11, 14, 20)
        } else {
            Rgba::rgb(248, 249, 251)
        };
        self.scene
            .set_layer(PCB_VIA_LAYER, Layer::new("Vias", gold));
        self.scene
            .set_layer(PCB_DRILL_LAYER, Layer::new("Drills", hole).not_snappable());
        for (id, t) in &pcb.tracks {
            self.scene.insert(
                shape_id(*id, 0),
                Item {
                    layer: pcb_layer_id(t.layer),
                    shape: Shape::Line {
                        segment: t.segment,
                        width: t.width,
                    },
                    color: None,
                },
            );
        }
        for (id, v) in &pcb.vias {
            self.scene.insert(
                shape_id(*id, 0),
                Item {
                    layer: PCB_VIA_LAYER,
                    shape: Shape::Circle {
                        circle: Circle {
                            center: v.position,
                            radius: v.diameter / 2,
                        },
                        filled: true,
                        stroke: 0,
                    },
                    color: None,
                },
            );
            self.scene.insert(
                shape_id(*id, 1),
                Item {
                    layer: PCB_DRILL_LAYER,
                    shape: Shape::Circle {
                        circle: Circle {
                            center: v.position,
                            radius: v.drill / 2,
                        },
                        filled: true,
                        stroke: 0,
                    },
                    color: None,
                },
            );
        }
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;
    use buturecad_schematic::{builtin, Component, Wire};

    const G: i64 = 1_270_000;

    fn schematic() -> Schematic {
        let mut s = Schematic::new(Id(0));
        builtin::install(&mut s);
        s
    }
    fn comp(lib: &str, x: i64, y: i64, rotation: u8, mirror: bool) -> Object {
        Object::Component(Component {
            lib_id: lib.into(),
            reference: "R1".into(),
            value: "10k".into(),
            footprint: String::new(),
            properties: Default::default(),
            unit: 1,
            representation: 1,
            placement: Placement {
                position: Point::new(x * G, y * G),
                rotation,
                mirror,
            },
        })
    }

    #[test]
    fn shape_ids_roundtrip_owners() {
        let id = shape_id(Id(12345), 7);
        assert_eq!(owner_of(id), Id(12345));
        assert_eq!(
            owner_of(shape_id(Id(1), 99_999)),
            Id(1),
            "overlong shape lists are clamped, not corrupted"
        );
    }

    #[test]
    fn component_has_pins_body_text_and_a_hit_area() {
        let s = schematic();
        let shapes = object_shapes(&s, &comp("Buture:R", 0, 0, 0, false));
        let on = |l: LayerId| shapes.iter().filter(|(layer, _)| *layer == l).count();
        assert_eq!((on(L_BODY), on(L_PIN), on(L_TEXT), on(L_HIT)), (1, 2, 2, 1));
    }

    #[test]
    fn pins_are_drawn_where_connectivity_expects_them() {
        let s = schematic();
        let obj = comp("Buture:R", 4, 6, 1, false);
        let Object::Component(c) = &obj else {
            unreachable!()
        };
        let pins: Vec<Point> = s.placed_pins(c).iter().map(|p| p.position).collect();
        let drawn: Vec<Point> = object_shapes(&s, &obj)
            .into_iter()
            .filter_map(|(l, sh)| match (l, sh) {
                (L_PIN, Shape::Line { segment, .. }) => Some(segment.a),
                _ => None,
            })
            .collect();
        assert_eq!(
            pins, drawn,
            "the drawn pin end must be the electrical connection point"
        );
    }

    #[test]
    fn sync_replaces_and_removes_shapes() {
        let s = schematic();
        let mut ss = SchematicScene::default();
        ss.rebuild(&s, ThemeKind::Dark);
        let w = Object::Wire(Wire {
            segment: Segment::new(Point::ORIGIN, Point::new(G, 0)),
        });
        ss.sync(&s, Id(5), Some(&w));
        assert_eq!(ss.shapes_of(Id(5)).len(), 1);
        let rev = ss.scene.revision();
        ss.sync(&s, Id(5), Some(&w));
        assert_eq!(ss.shapes_of(Id(5)).len(), 1, "no duplicates after re-sync");
        assert!(ss.scene.revision() > rev);
        ss.sync(&s, Id(5), None);
        assert!(ss.shapes_of(Id(5)).is_empty() && ss.scene.is_empty());
    }

    #[test]
    fn mirrored_arcs_keep_their_shape() {
        let s = schematic();
        let shapes = object_shapes(&s, &comp("Buture:L", 0, 0, 0, true));
        let arcs: Vec<_> = shapes
            .iter()
            .filter_map(|(_, sh)| {
                if let Shape::Arc { arc, .. } = sh {
                    Some(*arc)
                } else {
                    None
                }
            })
            .collect();
        assert_eq!(arcs.len(), 4);
        // Unmirrored bumps are on +x; mirrored they must be on -x (sweep includes direction -x).
        assert!(
            arcs.iter().all(|a| a.sweeps(Vector::new(-1, 0))),
            "mirror must flip bump side"
        );
    }

    #[test]
    fn pcb_scene_draws_tracks_and_vias() {
        use buturecad_eda_core::model::{Track, Via};
        let mut pcb = PcbDoc::default();
        pcb.tracks.insert(
            Id(1),
            Track {
                layer: PcbLayer(0),
                segment: Segment::new(Point::ORIGIN, Point::new(G, 0)),
                width: 250_000,
                net: None,
            },
        );
        pcb.vias.insert(
            Id(2),
            Via {
                position: Point::new(G, 0),
                diameter: 600_000,
                drill: 300_000,
                net: None,
                kind: Default::default(),
            },
        );
        let mut ps = PcbScene::default();
        ps.rebuild(&pcb, ThemeKind::Dark, &Default::default());
        assert_eq!(ps.scene.len(), 3);
        ps.rebuild(&pcb, ThemeKind::Dark, &[0u16].into_iter().collect());
        assert!(!ps.scene.is_visible(pcb_layer_id(PcbLayer(0))));
        assert_eq!(pcb_layer_name(PcbLayer(1)), "B.Cu");
    }
}
