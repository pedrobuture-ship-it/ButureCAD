//! DRC validation of a finished route.

use buturecad_eda_core::model::{Id, PcbDoc};
use buturecad_geometry::{Coord, Point, Segment};
use buturecad_pcb::LayerId;
use buturecad_rules::copper::{CShape, CopperItem, ItemKind};
use buturecad_rules::{candidate_edge_violations, check_candidate, DrcViolation};
use std::collections::BTreeSet;

/// New copper about to be committed.
pub struct RouteCandidate<'a> {
    pub net: Option<Id>,
    pub width: Coord,
    pub tracks: &'a [(LayerId, Vec<Point>)],
    pub vias: &'a [Point],
    pub via_diameter: Coord,
}

fn candidate_items(doc: &PcbDoc, c: &RouteCandidate<'_>) -> Vec<CopperItem> {
    let owner = Id(u64::MAX);
    let mut items = Vec::new();
    for (layer, poly) in c.tracks {
        for w in poly.windows(2) {
            items.push(CopperItem {
                owner,
                kind: ItemKind::Track,
                net: c.net,
                layer: *layer,
                shape: CShape::Cap {
                    seg: Segment::new(w[0], w[1]),
                    r: c.width as f64 / 2.0,
                },
                label: "the new track".into(),
            });
        }
    }
    for at in c.vias {
        for l in buturecad_eda_core::model::ViaKind::Through.layers(&doc.stackup) {
            items.push(CopperItem {
                owner,
                kind: ItemKind::Via,
                net: c.net,
                layer: l,
                shape: CShape::Cap {
                    seg: Segment::new(*at, *at),
                    r: c.via_diameter as f64 / 2.0,
                },
                label: "the new via".into(),
            });
        }
    }
    items
}

/// The clearance, short and board-edge violations the candidate would create. Empty means the
/// route is valid under the same rules the DRC panel applies.
pub fn validate_route(doc: &PcbDoc, c: &RouteCandidate<'_>) -> Vec<DrcViolation> {
    let items = candidate_items(doc, c);
    let mut v = check_candidate(doc, &items, &BTreeSet::new());
    v.extend(candidate_edge_violations(doc, &items));
    v
}

/// Validation of a differential pair about to be committed: each leg against the board, and the
/// legs against each other (they must keep the pair's gap and must not touch). Messages only.
pub fn validate_pair(
    doc: &PcbDoc,
    pos: &RouteCandidate<'_>,
    neg: &RouteCandidate<'_>,
) -> Vec<String> {
    let mut out: Vec<String> = validate_route(doc, pos)
        .into_iter()
        .map(|v| v.message)
        .collect();
    out.extend(validate_route(doc, neg).into_iter().map(|v| v.message));
    let (a, b) = (candidate_items(doc, pos), candidate_items(doc, neg));
    let required = doc.rules.clearance_between(pos.net, neg.net) as f64;
    'outer: for x in &a {
        for y in b.iter().filter(|y| y.layer == x.layer) {
            let d = buturecad_rules::copper::distance(&x.shape, &y.shape);
            if d + 1.0 < required {
                out.push(format!(
                    "the two legs of the pair are {:.3} mm apart on {}; {:.3} mm required",
                    d.max(0.0) / 1e6,
                    buturecad_pcb::layer::name(x.layer),
                    required / 1e6
                ));
                break 'outer;
            }
        }
    }
    out
}

/// Violations a replacement of the track `replace` by `path` would create (clearance, shorts and
/// board edge), ignoring the track being replaced. Used by length tuning.
pub fn validate_replacement(
    doc: &PcbDoc,
    replace: Id,
    layer: LayerId,
    net: Option<Id>,
    width: Coord,
    path: &[Point],
) -> Vec<DrcViolation> {
    let tracks = vec![(layer, path.to_vec())];
    let items = candidate_items(
        doc,
        &RouteCandidate {
            net,
            width,
            tracks: &tracks,
            vias: &[],
            via_diameter: 0,
        },
    );
    let mut v = check_candidate(doc, &items, &BTreeSet::from([replace]));
    v.extend(candidate_edge_violations(doc, &items));
    v
}
