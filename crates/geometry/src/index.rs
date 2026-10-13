//! R-tree spatial index over bounding boxes (see the crate docs for the rationale).

use crate::bbox::BoundingBox;
use crate::point::Point;
use crate::units::Coord;
use rstar::{RTree, RTreeObject, AABB};

struct Entry<T> {
    bbox: BoundingBox,
    item: T,
}

impl<T> RTreeObject for Entry<T> {
    type Envelope = AABB<[Coord; 2]>;
    fn envelope(&self) -> Self::Envelope {
        AABB::from_corners(
            [self.bbox.min.x, self.bbox.min.y],
            [self.bbox.max.x, self.bbox.max.y],
        )
    }
}

impl<T: PartialEq> PartialEq for Entry<T> {
    fn eq(&self, o: &Self) -> bool {
        self.bbox == o.bbox && self.item == o.item
    }
}

/// Dynamic spatial index mapping bounding boxes to items.
///
/// Queries return *candidates* whose boxes satisfy the box test; callers apply exact geometry
/// (e.g. `Segment::within_distance_of_point`) to the candidates.
pub struct SpatialIndex<T> {
    tree: RTree<Entry<T>>,
}

impl<T> Default for SpatialIndex<T> {
    fn default() -> Self {
        Self { tree: RTree::new() }
    }
}

impl<T> SpatialIndex<T> {
    pub fn new() -> Self {
        Self::default()
    }

    /// Builds an index in one pass (Sort-Tile-Recursive); faster and better balanced than
    /// repeated [`Self::insert`].
    pub fn bulk_load(items: impl IntoIterator<Item = (BoundingBox, T)>) -> Self {
        let entries = items
            .into_iter()
            .map(|(bbox, item)| Entry { bbox, item })
            .collect();
        Self {
            tree: RTree::bulk_load(entries),
        }
    }

    pub fn insert(&mut self, bbox: BoundingBox, item: T) {
        self.tree.insert(Entry { bbox, item });
    }

    pub fn len(&self) -> usize {
        self.tree.size()
    }

    pub fn is_empty(&self) -> bool {
        self.tree.size() == 0
    }

    /// Items whose box overlaps or touches `area`.
    pub fn query_intersecting<'a>(
        &'a self,
        area: &BoundingBox,
    ) -> impl Iterator<Item = (&'a BoundingBox, &'a T)> + 'a {
        let env = AABB::from_corners([area.min.x, area.min.y], [area.max.x, area.max.y]);
        self.tree
            .locate_in_envelope_intersecting(&env)
            .map(|e| (&e.bbox, &e.item))
    }

    /// Items whose box contains `p`.
    pub fn query_point(&self, p: Point) -> impl Iterator<Item = (&BoundingBox, &T)> + '_ {
        self.query_intersecting(&BoundingBox::from_point(p))
    }

    /// Candidates whose box lies within `d` of `p` along both axes (a superset of the items
    /// within Euclidean distance `d`).
    pub fn query_near(&self, p: Point, d: Coord) -> impl Iterator<Item = (&BoundingBox, &T)> + '_ {
        let area = BoundingBox::from_point(p)
            .inflate(d.max(0))
            .unwrap_or(BoundingBox::from_point(p));
        self.query_intersecting(&area)
    }
}

impl<T: PartialEq> SpatialIndex<T> {
    /// Removes one entry equal to `(bbox, item)`; returns whether it was present.
    pub fn remove(&mut self, bbox: &BoundingBox, item: T) -> bool {
        self.tree.remove(&Entry { bbox: *bbox, item }).is_some()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn bb(x0: i64, y0: i64, x1: i64, y1: i64) -> BoundingBox {
        BoundingBox::new(Point::new(x0, y0), Point::new(x1, y1))
    }

    #[test]
    fn insert_query_remove() {
        let mut idx = SpatialIndex::new();
        idx.insert(bb(0, 0, 10, 10), "a");
        idx.insert(bb(20, 20, 30, 30), "b");
        assert_eq!(idx.len(), 2);
        let hits: Vec<_> = idx
            .query_intersecting(&bb(5, 5, 25, 25))
            .map(|(_, v)| *v)
            .collect();
        assert_eq!(hits.len(), 2);
        assert_eq!(idx.query_point(Point::new(1, 1)).count(), 1);
        assert_eq!(idx.query_near(Point::new(15, 15), 5).count(), 2);
        assert!(idx.remove(&bb(0, 0, 10, 10), "a"));
        assert!(!idx.remove(&bb(0, 0, 10, 10), "a"));
        assert_eq!(idx.len(), 1);
        assert!(!idx.is_empty());
    }
}
