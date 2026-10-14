//! Routed length of nets.
//!
//! The length of a net is the sum of the lengths of its tracks (centre lines). Vias add nothing
//! and pad-to-pad geometry inside footprints is ignored: it is the length you can tune.

use crate::model::{Id, PcbDoc};
use std::collections::BTreeMap;

/// Routed length of `net` in nm (0 when it has no tracks).
pub fn net_length(doc: &PcbDoc, net: Id) -> f64 {
    doc.tracks
        .values()
        .filter(|t| t.net == Some(net))
        .map(|t| t.segment.length())
        .sum()
}

/// Routed length of every net that has tracks.
pub fn net_lengths(doc: &PcbDoc) -> BTreeMap<Id, f64> {
    let mut out: BTreeMap<Id, f64> = BTreeMap::new();
    for t in doc.tracks.values() {
        if let Some(n) = t.net {
            *out.entry(n).or_default() += t.segment.length();
        }
    }
    out
}

/// How a net's length stands against its goal.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct LengthStatus {
    pub current: f64,
    pub target: f64,
    pub tolerance: f64,
    /// The goal comes from the partner of a differential pair, not from a set target.
    pub from_pair: bool,
}

impl LengthStatus {
    /// Positive: too long; negative: too short (nm).
    pub fn difference(&self) -> f64 {
        self.current - self.target
    }
    pub fn within(&self) -> bool {
        self.difference().abs() <= self.tolerance
    }
}

/// The status of `net`, when it has routed copper and a goal (own target or pair partner).
pub fn length_status(doc: &PcbDoc, net: Id) -> Option<LengthStatus> {
    let current = net_length(doc, net);
    if current <= 0.0 {
        return None;
    }
    let (goal, from_pair) = doc.rules.effective_length_target(net, |n| {
        let l = net_length(doc, n);
        (l > 0.0).then_some(l.round() as i64)
    })?;
    Some(LengthStatus {
        current,
        target: goal.target as f64,
        tolerance: goal.tolerance as f64,
        from_pair,
    })
}
