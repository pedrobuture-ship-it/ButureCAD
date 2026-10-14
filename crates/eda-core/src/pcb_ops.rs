//! Pure geometric operations on board objects: translate, rotate, flip, bounds, align,
//! distribute, duplicate. Each returns new object states; the caller wraps them in
//! [`PcbChange`]s so they run through the Command Engine. Locked footprints are never modified.

use crate::model::{Id, PcbChange, PcbDoc, PcbObject};
use buturecad_geometry::{Coord, Point, Segment, Vector};
use buturecad_pcb::footprint_edit::map_shape;
use buturecad_pcb::{BoardGraphic, FpShape, Zone};
use thiserror::Error;

#[derive(Debug, Error, PartialEq, Eq, Clone)]
pub enum OpError {
    #[error("nothing to do: {0}")]
    Nothing(String),
    #[error("{0}")]
    Invalid(String),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Align {
    Left,
    Right,
    Top,
    Bottom,
    CenterH,
    CenterV,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Distribute {
    Horizontal,
    Vertical,
}

fn is_locked(o: &PcbObject) -> bool {
    matches!(o, PcbObject::Footprint(f) if f.locked)
}

fn shape_points(s: &FpShape) -> Vec<Point> {
    match s {
        FpShape::Line { segment, .. } => vec![segment.a, segment.b],
        FpShape::Rect { rect, .. } => vec![rect.min(), rect.max()],
        FpShape::Circle { circle, .. } => vec![
            Point::new(
                circle.center.x - circle.radius,
                circle.center.y - circle.radius,
            ),
            Point::new(
                circle.center.x + circle.radius,
                circle.center.y + circle.radius,
            ),
        ],
        FpShape::Arc { arc, .. } => vec![arc.center, arc.start, arc.end],
        FpShape::Polygon { points, .. } => points.clone(),
    }
}

fn bbox(pts: &[Point], pad: Coord) -> Option<(Point, Point)> {
    let lo = Point::new(
        pts.iter().map(|p| p.x).min()? - pad,
        pts.iter().map(|p| p.y).min()? - pad,
    );
    let hi = Point::new(
        pts.iter().map(|p| p.x).max()? + pad,
        pts.iter().map(|p| p.y).max()? + pad,
    );
    Some((lo, hi))
}

/// Bounds `(min, max)` of an object in board coordinates.
pub fn bounds(doc: &PcbDoc, o: &PcbObject) -> Option<(Point, Point)> {
    match o {
        PcbObject::Track(t) => bbox(&[t.segment.a, t.segment.b], t.width / 2),
        PcbObject::Via(v) => bbox(&[v.position], v.diameter / 2),
        PcbObject::Footprint(f) => f.bounds(doc.footprints.get(&f.footprint)?),
        PcbObject::Net(_) => None,
        PcbObject::Zone(z) => bbox(&z.outline, 0),
        PcbObject::Keepout(k) => bbox(&k.outline, 0),
        PcbObject::Text(t) => {
            let w = t.text.chars().count() as Coord * 6 * (t.height / 7);
            bbox(
                &[
                    t.position,
                    Point::new(t.position.x + w, t.position.y + t.height),
                ],
                0,
            )
        }
        PcbObject::Graphic(g) => bbox(&shape_points(&g.shape), 0),
    }
}

/// Applies a point map to every coordinate of `o`. `flips` is true when the map mirrors (arcs
/// reverse their sweep). Footprints are handled by the callers (they need placement logic).
fn map_geometry(o: &PcbObject, f: &impl Fn(Point) -> Point, flips: bool) -> Option<PcbObject> {
    Some(match o {
        PcbObject::Track(t) => {
            let mut t = t.clone();
            t.segment = Segment::new(f(t.segment.a), f(t.segment.b));
            PcbObject::Track(t)
        }
        PcbObject::Via(v) => {
            let mut v = v.clone();
            v.position = f(v.position);
            PcbObject::Via(v)
        }
        // A moved zone's old fill no longer matches: drop it (it is refilled on demand).
        PcbObject::Zone(z) => PcbObject::Zone(Zone {
            outline: z.outline.iter().map(|p| f(*p)).collect(),
            fill: None,
            ..z.clone()
        }),
        PcbObject::Keepout(k) => {
            let mut k = k.clone();
            k.outline = k.outline.iter().map(|p| f(*p)).collect();
            PcbObject::Keepout(k)
        }
        PcbObject::Text(t) => {
            let mut t = t.clone();
            t.position = f(t.position);
            PcbObject::Text(t)
        }
        PcbObject::Graphic(g) => PcbObject::Graphic(BoardGraphic {
            layer: g.layer,
            shape: map_shape(&g.shape, f, flips)?,
        }),
        PcbObject::Footprint(_) | PcbObject::Net(_) => return None,
    })
}

pub fn translate(o: &PcbObject, d: Vector) -> Option<PcbObject> {
    if is_locked(o) {
        return None;
    }
    if let PcbObject::Footprint(fp) = o {
        let mut fp = fp.clone();
        fp.placement.position = fp.placement.position + d;
        return Some(PcbObject::Footprint(fp));
    }
    map_geometry(o, &|p| p + d, false)
}

/// 90° counter-clockwise about `pivot`.
pub fn rotate(o: &PcbObject, pivot: Point) -> Option<PcbObject> {
    if is_locked(o) {
        return None;
    }
    if let PcbObject::Footprint(fp) = o {
        let mut fp = fp.clone();
        fp.placement.position = fp.placement.position.rotate_quarter_turns(pivot, 1);
        fp.placement.rotation = (fp.placement.rotation + 1) % 4;
        return Some(PcbObject::Footprint(fp));
    }
    let mut out = map_geometry(o, &|p| p.rotate_quarter_turns(pivot, 1), false)?;
    if let PcbObject::Text(t) = &mut out {
        t.rotation = (t.rotation + 1) % 4;
    }
    Some(out)
}

/// Flips a footprint to the other side about its own origin (mirror + layer swap).
pub fn flip_footprint(o: &PcbObject) -> Option<PcbObject> {
    match o {
        PcbObject::Footprint(fp) if !fp.locked => {
            let mut fp = fp.clone();
            fp.placement.flipped = !fp.placement.flipped;
            fp.placement.rotation = (4 - fp.placement.rotation % 4) % 4;
            Some(PcbObject::Footprint(fp))
        }
        _ => None,
    }
}

/// Moves the objects so their edges / centres line up. Returns one change per moved object.
pub fn align(doc: &PcbDoc, ids: &[Id], mode: Align) -> Result<Vec<PcbChange>, OpError> {
    let items: Vec<(Id, PcbObject, (Point, Point))> = ids
        .iter()
        .filter_map(|id| {
            let o = doc.object(*id)?;
            let b = bounds(doc, &o)?;
            Some((*id, o, b))
        })
        .collect();
    if items.len() < 2 {
        return Err(OpError::Nothing(
            "select at least two objects to align".into(),
        ));
    }
    let min_x = items.iter().map(|i| i.2 .0.x).min().unwrap_or(0);
    let max_x = items.iter().map(|i| i.2 .1.x).max().unwrap_or(0);
    let min_y = items.iter().map(|i| i.2 .0.y).min().unwrap_or(0);
    let max_y = items.iter().map(|i| i.2 .1.y).max().unwrap_or(0);
    let mut out = Vec::new();
    for (id, o, (lo, hi)) in items {
        let d = match mode {
            Align::Left => Vector::new(min_x - lo.x, 0),
            Align::Right => Vector::new(max_x - hi.x, 0),
            Align::Bottom => Vector::new(0, min_y - lo.y),
            Align::Top => Vector::new(0, max_y - hi.y),
            Align::CenterH => Vector::new((min_x + max_x) / 2 - (lo.x + hi.x) / 2, 0),
            Align::CenterV => Vector::new(0, (min_y + max_y) / 2 - (lo.y + hi.y) / 2),
        };
        if d.is_zero() {
            continue;
        }
        if let Some(n) = translate(&o, d) {
            out.push(PcbChange::modify(id, o, n));
        }
    }
    if out.is_empty() {
        return Err(OpError::Nothing(
            "already aligned (or the objects are locked)".into(),
        ));
    }
    Ok(out)
}

/// Spaces the object *centres* evenly between the outermost two.
pub fn distribute(doc: &PcbDoc, ids: &[Id], mode: Distribute) -> Result<Vec<PcbChange>, OpError> {
    let mut items: Vec<(Id, PcbObject, Point)> = ids
        .iter()
        .filter_map(|id| {
            let o = doc.object(*id)?;
            let (lo, hi) = bounds(doc, &o)?;
            Some((*id, o, Point::new((lo.x + hi.x) / 2, (lo.y + hi.y) / 2)))
        })
        .collect();
    if items.len() < 3 {
        return Err(OpError::Nothing(
            "select at least three objects to distribute".into(),
        ));
    }
    let key = |c: &Point| match mode {
        Distribute::Horizontal => c.x,
        Distribute::Vertical => c.y,
    };
    items.sort_by_key(|i| (key(&i.2), i.0));
    let (first, last) = (key(&items[0].2), key(&items[items.len() - 1].2));
    let n = (items.len() - 1) as i128;
    let mut out = Vec::new();
    for (i, (id, o, c)) in items.into_iter().enumerate() {
        let target = first + ((i128::from(last - first) * i as i128 + n / 2) / n) as Coord;
        let d = match mode {
            Distribute::Horizontal => Vector::new(target - c.x, 0),
            Distribute::Vertical => Vector::new(0, target - c.y),
        };
        if d.is_zero() {
            continue;
        }
        if let Some(m) = translate(&o, d) {
            out.push(PcbChange::modify(id, o, m));
        }
    }
    if out.is_empty() {
        return Err(OpError::Nothing(
            "already evenly distributed (or locked)".into(),
        ));
    }
    Ok(out)
}

/// Copies of the objects offset by `offset`. Footprints keep their definition but lose their
/// schematic link and nets (a copy is a new, unconnected part); tracks and vias lose their net.
/// `alloc` hands out new ids.
pub fn duplicate(
    doc: &PcbDoc,
    ids: &[Id],
    offset: Vector,
    mut alloc: impl FnMut() -> Id,
) -> Vec<PcbChange> {
    let mut out = Vec::new();
    for id in ids {
        let Some(o) = doc.object(*id) else { continue };
        let copy = match &o {
            PcbObject::Footprint(fp) => {
                let mut c = fp.clone();
                c.placement.position = c.placement.position + offset;
                c.locked = false;
                c.link = None;
                c.pad_nets.clear();
                c.reference = match c
                    .reference
                    .trim_end_matches(|ch: char| ch.is_ascii_digit() || ch == '?')
                {
                    "" => "REF**".to_owned(),
                    prefix => format!("{prefix}?"),
                };
                PcbObject::Footprint(c)
            }
            PcbObject::Net(_) => continue,
            other => match translate(other, offset) {
                Some(PcbObject::Track(mut t)) => {
                    t.net = None;
                    PcbObject::Track(t)
                }
                Some(PcbObject::Via(mut v)) => {
                    v.net = None;
                    PcbObject::Via(v)
                }
                Some(PcbObject::Zone(mut z)) => {
                    z.net = None;
                    PcbObject::Zone(z)
                }
                Some(x) => x,
                None => continue,
            },
        };
        out.push(PcbChange::create(alloc(), copy));
    }
    out
}

/// Closed-outline check of the `Edge.Cuts` graphics: every segment end must meet exactly one
/// other end. Returns human-readable problems (empty = closed outline or no outline).
pub fn outline_problems(doc: &PcbDoc) -> Vec<String> {
    use buturecad_pcb::layer::EDGE_CUTS;
    let mut ends: std::collections::BTreeMap<Point, u32> = std::collections::BTreeMap::new();
    let mut count = 0;
    for g in doc.graphics.values().filter(|g| g.layer == EDGE_CUTS) {
        let pts: Vec<(Point, Point)> = match &g.shape {
            FpShape::Line { segment, .. } => vec![(segment.a, segment.b)],
            FpShape::Arc { arc, .. } => vec![(arc.start, arc.end)],
            FpShape::Polygon { points, .. } => points
                .iter()
                .enumerate()
                .map(|(i, p)| (*p, points[(i + 1) % points.len()]))
                .collect(),
            FpShape::Rect { .. } | FpShape::Circle { .. } => continue,
        };
        for (a, b) in pts {
            count += 1;
            *ends.entry(a).or_default() += 1;
            *ends.entry(b).or_default() += 1;
        }
    }
    let mut out = Vec::new();
    if count > 0 && ends.values().any(|n| *n != 2) {
        out.push("The board outline (Edge.Cuts) is not a closed loop".to_owned());
    }
    out
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::panic)]
mod tests {
    use super::*;
    use crate::model::*;
    use buturecad_pcb::generators::Soic;
    use buturecad_pcb::{layer, PlacedFootprint, Placement};
    use std::collections::BTreeMap;

    fn doc_with(n: usize) -> (PcbDoc, Vec<Id>) {
        let mut d = PcbDoc::default();
        let def = Soic::narrow(8).generate().unwrap();
        let name = def.name.clone();
        d.footprints.insert(name.clone(), def);
        let mut ids = Vec::new();
        for i in 0..n {
            let id = Id(10 + i as u64);
            d.placed.insert(
                id,
                PlacedFootprint {
                    footprint: name.clone(),
                    reference: format!("U{}", i + 1),
                    value: String::new(),
                    placement: Placement::at(Point::new(
                        (i as i64) * 20_000_000 + (i as i64 * i as i64) * 1_000_000,
                        (i as i64 % 2) * 3_000_000,
                    )),
                    locked: false,
                    link: Some(Id(99)),
                    pad_nets: BTreeMap::new(),
                },
            );
            ids.push(id);
        }
        (d, ids)
    }

    fn apply(d: &mut PcbDoc, c: &[PcbChange]) {
        d.apply_changes(c).unwrap();
    }

    #[test]
    fn translate_rotate_flip_respect_locks_and_roundtrip() {
        let (d, ids) = doc_with(1);
        let o = d.object(ids[0]).unwrap();
        let mut cur = o.clone();
        for _ in 0..4 {
            cur = rotate(&cur, Point::new(5_000_000, 5_000_000)).unwrap();
        }
        assert_eq!(cur, o);
        let flipped = flip_footprint(&o).unwrap();
        assert_eq!(flip_footprint(&flipped).unwrap(), o);
        let PcbObject::Footprint(mut f) = o.clone() else {
            panic!()
        };
        f.locked = true;
        let locked = PcbObject::Footprint(f);
        assert!(translate(&locked, Vector::new(1, 1)).is_none());
        assert!(rotate(&locked, Point::ORIGIN).is_none());
        assert!(flip_footprint(&locked).is_none());
        // Tracks have no flip.
        let t = PcbObject::Track(Track {
            layer: layer::F_CU,
            segment: Segment::new(Point::ORIGIN, Point::new(10, 0)),
            width: 2,
            net: None,
        });
        assert!(flip_footprint(&t).is_none());
        let r = rotate(&t, Point::ORIGIN).unwrap();
        assert_eq!(
            r,
            PcbObject::Track(Track {
                layer: layer::F_CU,
                segment: Segment::new(Point::ORIGIN, Point::new(0, 10)),
                width: 2,
                net: None
            })
        );
    }

    #[test]
    fn align_and_distribute_move_the_right_edges() {
        let (mut d, ids) = doc_with(4);
        let c = align(&d, &ids, Align::Left).unwrap();
        apply(&mut d, &c);
        let lefts: Vec<i64> = ids
            .iter()
            .map(|i| bounds(&d, &d.object(*i).unwrap()).unwrap().0.x)
            .collect();
        assert!(lefts.windows(2).all(|w| w[0] == w[1]), "{lefts:?}");
        let c = align(&d, &ids, Align::Top).unwrap();
        apply(&mut d, &c);
        let tops: Vec<i64> = ids
            .iter()
            .map(|i| bounds(&d, &d.object(*i).unwrap()).unwrap().1.y)
            .collect();
        assert!(tops.windows(2).all(|w| w[0] == w[1]));
        assert!(align(&d, &ids, Align::Top).is_err(), "already aligned");
        assert!(align(&d, &ids[..1], Align::Left).is_err());

        // Distribute horizontally: centres end up evenly spaced.
        let (mut d, ids) = doc_with(4);
        let c = distribute(&d, &ids, Distribute::Horizontal).unwrap();
        apply(&mut d, &c);
        let mid = |i: &Id| {
            let (lo, hi) = bounds(&d, &d.object(*i).unwrap()).unwrap();
            (lo.x + hi.x) / 2
        };
        let m: Vec<i64> = ids.iter().map(mid).collect();
        let gaps: Vec<i64> = m.windows(2).map(|w| w[1] - w[0]).collect();
        assert!(gaps.iter().all(|g| (g - gaps[0]).abs() <= 1), "{gaps:?}");
        assert!(distribute(&d, &ids[..2], Distribute::Vertical).is_err());
    }

    #[test]
    fn locked_footprints_do_not_move_when_aligning() {
        let (mut d, ids) = doc_with(3);
        d.placed.get_mut(&ids[2]).unwrap().locked = true;
        let before = d.placed[&ids[2]].clone();
        let c = align(&d, &ids, Align::Left).unwrap();
        assert!(c.iter().all(|x| x.id != ids[2]));
        apply(&mut d, &c);
        assert_eq!(d.placed[&ids[2]], before);
    }

    #[test]
    fn duplicates_are_new_unconnected_parts() {
        let (d, ids) = doc_with(1);
        let mut next = 100;
        let c = duplicate(&d, &ids, Vector::new(5_000_000, 0), || {
            next += 1;
            Id(next)
        });
        assert_eq!(c.len(), 1);
        let Some(PcbObject::Footprint(f)) = &c[0].after else {
            panic!()
        };
        assert_eq!(
            (f.reference.as_str(), f.link, f.pad_nets.len()),
            ("U?", None, 0)
        );
        assert_eq!(
            f.placement.position.x,
            d.placed[&ids[0]].placement.position.x + 5_000_000
        );
    }

    #[test]
    fn open_outlines_are_reported() {
        let mut d = PcbDoc::default();
        let line = |a: (i64, i64), b: (i64, i64)| BoardGraphic {
            layer: layer::EDGE_CUTS,
            shape: FpShape::Line {
                segment: Segment::new(Point::new(a.0, a.1), Point::new(b.0, b.1)),
                width: 100_000,
            },
        };
        assert!(outline_problems(&d).is_empty());
        d.graphics.insert(Id(1), line((0, 0), (10, 0)));
        d.graphics.insert(Id(2), line((10, 0), (10, 10)));
        d.graphics.insert(Id(3), line((10, 10), (0, 10)));
        assert_eq!(outline_problems(&d).len(), 1);
        d.graphics.insert(Id(4), line((0, 10), (0, 0)));
        assert!(outline_problems(&d).is_empty());
    }
}
