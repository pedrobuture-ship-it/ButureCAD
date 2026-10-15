//! The audit log: every API call becomes a structured, serialisable entry.

use serde::{Deserialize, Serialize};
use serde_json::Value;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct AuditEntry {
    pub seq: u64,
    /// `read`, `analyze`, `propose`, `validated`, `invalid`, `approved`, `applied`, `rejected`,
    /// `apply_failed`.
    pub kind: String,
    /// The agent or user behind the entry.
    pub actor: String,
    pub proposal: Option<String>,
    /// What was asked / what happened, structured.
    pub detail: Value,
}

#[derive(Debug, Clone, Default, PartialEq)]
pub struct AuditLog {
    entries: Vec<AuditEntry>,
}

impl AuditLog {
    pub fn record(
        &mut self,
        kind: &str,
        actor: &str,
        proposal: Option<&str>,
        detail: Value,
    ) -> &AuditEntry {
        let seq = self.entries.len() as u64 + 1;
        self.entries.push(AuditEntry {
            seq,
            kind: kind.to_owned(),
            actor: actor.to_owned(),
            proposal: proposal.map(str::to_owned),
            detail,
        });
        &self.entries[self.entries.len() - 1]
    }

    pub fn entries(&self) -> &[AuditEntry] {
        &self.entries
    }

    /// One JSON object per line (JSON Lines), ready to append to a file.
    pub fn to_jsonl(&self) -> String {
        self.entries
            .iter()
            .filter_map(|e| serde_json::to_string(e).ok())
            .map(|l| l + "\n")
            .collect()
    }
}
