//! The obstacles of one route: everything the new copper must keep clear of.

use crate::collision::{convex_hull, point_inside, segment_crosses_interior};
use buturecad_eda_core::model::{Id, PcbDoc};
use buturecad_geometry::{BoundingBox, Coord, Point, Segment, SpatialIndex};
use buturecad_pcb::LayerId;
use buturecad_rules::copper::{self, CShape};
use std::f64::consts::PI;

/// What is being routed.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RouteParams {
    pub net: Option<Id>,
    /// The second net when a differential pair is routed as one thick track (its copper is not
    /// an obstacle either).
    pub also_net: Option<Id>,
    pub layer: LayerId,
    /// Width of the routed copper: a track's width, or the whole pair (2 × width + gap).
    pub width: Coord,
}

impl RouteParams {
    pub fn new(net: Option<Id>, layer: LayerId, width: Coord) -> Self {
        Self {
            net,
            also_net: None,
            layer,
            width,
        }
    }
}

/// A convex obstacle (counter-clockwise hull) with the board object it came from.
#[derive(Debug, Clone)]
pub struct Obstacle {
    pub hull: Vec<Point>,
    pub owner: Id,
    pub bbox: BoundingBox,
}

/// Obstacles for one `(net, layer, width)`, indexed for fast segment queries.
pub struct ObstacleSet {
    pub params: RouteParams,
    obstacles: Vec<Obstacle>,
    index: SpatialIndex<usize>,
}

const RING: usize = 16;

/// Points of a regular `RING`-gon **circumscribed** around the circle `(c, r)`, so the polygon
/// contains the circle and the clearance can only be over-estimated, never under-estimated.
fn ring(c: Point, r: f64, out: &mut Vec<Point>) {
    let big = r / (PI / RING as f64).cos() + 2.0;
    for k in 0..RING {
        let a = 2.0 * PI * k as f64 / RING as f64;
        out.push(Point::new(
            (c.x as f64 + big * a.cos()).round() as i64,
            (c.y as f64 + big * a.sin()).round() as i64,
        ));
    }
}

fn bbox_of(hull: &[Point]) -> BoundingBox {
    let mut lo = hull[0];
    let mut hi = hull[0];
    for p in hull {
        lo = Point::new(lo.x.min(p.x), lo.y.min(p.y));
        hi = Point::new(hi.x.max(p.x), hi.y.max(p.y));
    }
    BoundingBox::new(lo, hi)
}

/// Convex obstacle covering everything within `d` of `base` points (`d` = radius around each).
fn inflate(base: &[Point], d: f64) -> Vec<Point> {
    let mut pts = Vec::with_capacity(base.len() * RING);
    for b in base {
        ring(*b, d, &mut pts);
    }
    convex_hull(&pts)
}

impl ObstacleSet {
    /// Collects the obstacles for `params` on `doc`: copper of other nets (and of unassigned
    /// copper), keepouts that forbid tracks, and the board edge.
    pub fn collect(doc: &PcbDoc, params: RouteParams) -> Self {
        Self::collect_with(doc, params, true)
    }

    /// Like [`Self::collect`]; with `include_tracks == false` the tracks of other nets are left
    /// out (Push & Shove treats them as movable, not as walls).
    pub fn collect_with(doc: &PcbDoc, params: RouteParams, include_tracks: bool) -> Self {
        let mut obstacles = Vec::new();
        let half = params.width as f64 / 2.0;
        for id in doc.ids() {
            if !include_tracks && doc.tracks.contains_key(&id) {
                continue;
            }
            for it in copper::items_of(doc, id) {
                // Zones are refilled around the new copper, they are not obstacles.
                if it.kind == buturecad_rules::copper::ItemKind::Zone {
                    continue;
                }
                if it.layer != params.layer
                    || (params.net.is_some() && it.net == params.net)
                    || (params.also_net.is_some() && it.net == params.also_net)
                {
                    continue;
                }
                let clearance = doc
                    .rules
                    .clearance_between(params.net, it.net)
                    .max(doc.rules.clearance_between(params.also_net, it.net));
                let d = clearance as f64 + half;
                let hull = match &it.shape {
                    CShape::Cap { seg, r } => inflate(&[seg.a, seg.b], r + d),
                    CShape::Poly(p) => inflate(p.vertices(), d),
                    CShape::Region(_) => continue,
                };
                if hull.len() >= 3 {
                    obstacles.push(Obstacle {
                        bbox: bbox_of(&hull),
                        hull,
                        owner: id,
                    });
                }
            }
        }
        for k in doc.keepouts.values() {
            let hits_layer = k.layers.is_empty() || k.layers.contains(&params.layer);
            if k.no_tracks && hits_layer && k.outline.len() >= 3 {
                let hull = inflate(&k.outline, half);
                if hull.len() >= 3 {
                    obstacles.push(Obstacle {
                        bbox: bbox_of(&hull),
                        hull,
                        owner: Id(0),
                    });
                }
            }
        }
        let edge = doc.rules.copper_to_edge as f64 + half;
        for s in buturecad_rules::outline_segments(doc) {
            let hull = inflate(&[s.a, s.b], edge);
            obstacles.push(Obstacle {
                bbox: bbox_of(&hull),
                hull,
                owner: Id(0),
            });
        }
        let index = SpatialIndex::bulk_load(obstacles.iter().enumerate().map(|(i, o)| (o.bbox, i)));
        Self {
            params,
            obstacles,
            index,
        }
    }

    pub fn len(&self) -> usize {
        self.obstacles.len()
    }

    pub fn is_empty(&self) -> bool {
        self.obstacles.is_empty()
    }

    pub fn obstacles(&self) -> &[Obstacle] {
        &self.obstacles
    }

    /// Indices of obstacles whose box touches `area`.
    pub fn in_area(&self, area: &BoundingBox) -> Vec<usize> {
        self.index
            .query_intersecting(area)
            .map(|(_, i)| *i)
            .collect()
    }

    /// The obstacle `p` is strictly inside, if any.
    pub fn containing(&self, p: Point) -> Option<&Obstacle> {
        let probe = BoundingBox::new(p, p);
        self.index
            .query_intersecting(&probe)
            .map(|(_, i)| &self.obstacles[*i])
            .find(|o| point_inside(&o.hull, p))
    }

    /// The first obstacle the segment passes through, if any.
    pub fn blocking(&self, a: Point, b: Point) -> Option<&Obstacle> {
        let area = BoundingBox::new(
            Point::new(a.x.min(b.x), a.y.min(b.y)),
            Point::new(a.x.max(b.x), a.y.max(b.y)),
        );
        self.index
            .query_intersecting(&area)
            .map(|(_, i)| &self.obstacles[*i])
            .find(|o| segment_crosses_interior(&o.hull, a, b))
    }

    /// The first obstacle a polyline passes through, if any.
    pub fn blocking_path(&self, pts: &[Point]) -> Option<&Obstacle> {
        pts.windows(2).find_map(|w| self.blocking(w[0], w[1]))
    }

    /// Is the segment free of obstacles?
    pub fn is_free(&self, s: &Segment) -> bool {
        self.blocking(s.a, s.b).is_none()
    }
}
