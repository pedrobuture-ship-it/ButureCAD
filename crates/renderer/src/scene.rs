//! The retained scene: layers and shapes, with a spatial index for picking and culling.

use crate::font;
use buturecad_geometry::{
    Arc, Bounded, BoundingBox, Circle, Coord, Point, Polygon, Rectangle, Segment, SpatialIndex,
    Vector,
};
use std::collections::BTreeMap;
use std::sync::atomic::{AtomicU64, Ordering};

/// Revisions are drawn from one global counter, so two different scenes never report the same
/// revision (a GPU cache keyed by revision must not mistake a rebuilt scene for the old one).
static NEXT_REVISION: AtomicU64 = AtomicU64::new(1);

fn next_revision() -> u64 {
    NEXT_REVISION.fetch_add(1, Ordering::Relaxed)
}

/// Opaque RGBA colour, sRGB values as in CSS.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct Rgba(pub [u8; 4]);

impl Rgba {
    pub const fn rgb(r: u8, g: u8, b: u8) -> Self {
        Rgba([r, g, b, 255])
    }
    pub const fn with_alpha(r: u8, g: u8, b: u8, a: u8) -> Self {
        Rgba([r, g, b, a])
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct ObjectId(pub u64);

/// Layer identifier. Layers are drawn in ascending id order (higher ids on top).
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct LayerId(pub u16);

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Layer {
    pub name: String,
    pub color: Rgba,
    pub visible: bool,
    /// Pickable but never drawn (e.g. invisible hit areas for filled click targets).
    pub hit_only: bool,
    /// Whether object snapping considers this layer's characteristic points.
    pub snappable: bool,
    /// Whether picking and box selection can hit this layer (false: drawn but inert, e.g. the
    /// fill of a copper zone, which must not swallow clicks meant for what lies on top of it).
    pub pickable: bool,
}

impl Layer {
    pub fn new(name: impl Into<String>, color: Rgba) -> Self {
        Self {
            name: name.into(),
            color,
            visible: true,
            hit_only: false,
            snappable: true,
            pickable: true,
        }
    }
    pub fn with_visible(mut self, visible: bool) -> Self {
        self.visible = visible;
        self
    }
    pub fn hit_only(mut self) -> Self {
        self.hit_only = true;
        self.snappable = false;
        self
    }
    /// Drawn, but never picked or snapped to.
    pub fn draw_only(mut self) -> Self {
        self.pickable = false;
        self.snappable = false;
        self
    }
    pub fn not_snappable(mut self) -> Self {
        self.snappable = false;
        self
    }
}

/// Drawable geometry. Widths are in nm; a width of 0 is drawn as a 1 px hairline.
#[derive(Debug, Clone, PartialEq)]
pub enum Shape {
    Line {
        segment: Segment,
        width: Coord,
    },
    Arc {
        arc: Arc,
        width: Coord,
    },
    Circle {
        circle: Circle,
        filled: bool,
        stroke: Coord,
    },
    Rect {
        rect: Rectangle,
        filled: bool,
        stroke: Coord,
    },
    Polygon {
        polygon: Polygon,
        filled: bool,
        stroke: Coord,
    },
    /// Single-line text, baseline-left anchored at `origin`; `height` is the cap height in nm.
    Text {
        origin: Point,
        height: Coord,
        text: String,
    },
}

impl Shape {
    /// Bounding box including stroke width.
    pub fn bounding_box(&self) -> BoundingBox {
        let grow = |bb: BoundingBox, w: Coord| bb.inflate(w / 2 + 1).unwrap_or(bb);
        match self {
            Shape::Line { segment, width } => grow(segment.bounding_box(), *width),
            Shape::Arc { arc, width } => grow(arc.bounding_box(), *width),
            Shape::Circle { circle, stroke, .. } => grow(circle.bounding_box(), *stroke),
            Shape::Rect { rect, stroke, .. } => grow(rect.bounding_box(), *stroke),
            Shape::Polygon {
                polygon, stroke, ..
            } => grow(polygon.bounding_box(), *stroke),
            Shape::Text {
                origin,
                height,
                text,
            } => {
                let (w, h) = font::text_extent(text, *height);
                BoundingBox::new(*origin, *origin + Vector::new(w, h))
            }
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct Item {
    pub layer: LayerId,
    pub shape: Shape,
    /// Overrides the layer colour (e.g. to show selection state of a pad).
    pub color: Option<Rgba>,
}

/// Retained scene. Mutations bump [`Scene::revision`], which the GPU backend uses to know when
/// cached meshes are stale.
pub struct Scene {
    layers: BTreeMap<LayerId, Layer>,
    items: BTreeMap<ObjectId, Item>,
    index: SpatialIndex<ObjectId>,
    boxes: BTreeMap<ObjectId, BoundingBox>,
    revision: u64,
}

impl Default for Scene {
    fn default() -> Self {
        Self {
            layers: BTreeMap::new(),
            items: BTreeMap::new(),
            index: SpatialIndex::new(),
            boxes: BTreeMap::new(),
            revision: next_revision(),
        }
    }
}

impl Scene {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn revision(&self) -> u64 {
        self.revision
    }
    pub fn len(&self) -> usize {
        self.items.len()
    }
    pub fn is_empty(&self) -> bool {
        self.items.is_empty()
    }

    pub fn set_layer(&mut self, id: LayerId, layer: Layer) {
        self.layers.insert(id, layer);
        self.revision = next_revision();
    }
    pub fn layer(&self, id: LayerId) -> Option<&Layer> {
        self.layers.get(&id)
    }
    pub fn layers(&self) -> impl Iterator<Item = (LayerId, &Layer)> {
        self.layers.iter().map(|(k, v)| (*k, v))
    }
    pub fn set_layer_visible(&mut self, id: LayerId, visible: bool) {
        if let Some(l) = self.layers.get_mut(&id) {
            if l.visible != visible {
                l.visible = visible;
                self.revision = next_revision();
            }
        }
    }
    pub fn is_visible(&self, layer: LayerId) -> bool {
        self.layers.get(&layer).is_some_and(|l| l.visible)
    }

    /// Inserts or replaces an object.
    pub fn insert(&mut self, id: ObjectId, item: Item) {
        self.remove(id);
        let bb = item.shape.bounding_box();
        self.index.insert(bb, id);
        self.boxes.insert(id, bb);
        self.items.insert(id, item);
        self.revision = next_revision();
    }

    pub fn remove(&mut self, id: ObjectId) -> Option<Item> {
        let item = self.items.remove(&id)?;
        if let Some(bb) = self.boxes.remove(&id) {
            self.index.remove(&bb, id);
        }
        self.revision = next_revision();
        Some(item)
    }

    pub fn clear_items(&mut self) {
        self.items.clear();
        self.boxes.clear();
        self.index = SpatialIndex::new();
        self.revision = next_revision();
    }

    pub fn get(&self, id: ObjectId) -> Option<&Item> {
        self.items.get(&id)
    }
    pub fn items(&self) -> impl Iterator<Item = (ObjectId, &Item)> {
        self.items.iter().map(|(k, v)| (*k, v))
    }
    pub fn bounding_box_of(&self, id: ObjectId) -> Option<BoundingBox> {
        self.boxes.get(&id).copied()
    }

    /// Bounding box of every visible object.
    pub fn bounds(&self) -> Option<BoundingBox> {
        let mut it = self
            .items
            .iter()
            .filter(|(_, i)| self.is_visible(i.layer))
            .filter_map(|(id, _)| self.boxes.get(id));
        let first = *it.next()?;
        Some(it.fold(first, |a, b| a.union(b)))
    }

    /// Candidate ids whose bounding box overlaps `area` (visible layers only).
    pub fn query(&self, area: &BoundingBox) -> impl Iterator<Item = ObjectId> + '_ {
        self.index
            .query_intersecting(area)
            .map(|(_, id)| *id)
            .filter(|id| {
                self.items.get(id).is_some_and(|i| {
                    self.is_visible(i.layer) && self.layers.get(&i.layer).is_none_or(|l| l.pickable)
                })
            })
    }

    /// Resolved colour of an item.
    pub fn color_of(&self, item: &Item) -> Rgba {
        item.color
            .or_else(|| self.layers.get(&item.layer).map(|l| l.color))
            .unwrap_or(Rgba::rgb(255, 0, 255))
    }
}
