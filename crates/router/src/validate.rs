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

/// The clearance, short and board-edge violations the candidate would create. Empty means the
/// route is valid under the same rules the DRC panel applies.
pub fn validate_route(doc: &PcbDoc, c: &RouteCandidate<'_>) -> Vec<DrcViolation> {
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
    let mut v = check_candidate(doc, &items, &BTreeSet::new());
    v.extend(candidate_edge_violations(doc, &items));
    v
}
