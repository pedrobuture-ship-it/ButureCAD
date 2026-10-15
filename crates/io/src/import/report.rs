//! The Import Report: converted / ignored / unsupported / warnings / errors.

use serde::Serialize;
use serde_json::{json, Value};

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct ReportItem {
    /// Kind of record (`component`, `net`, `directive`, `comment`…).
    pub kind: String,
    pub name: String,
    /// Why it was ignored / not supported, or what it became.
    pub detail: String,
    /// How many records this line stands for (comments are summarised).
    pub count: usize,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize)]
pub struct ImportReport {
    pub format: String,
    pub source: String,
    /// Records read from the file.
    pub records_seen: usize,
    pub converted: Vec<ReportItem>,
    pub ignored: Vec<ReportItem>,
    pub unsupported: Vec<ReportItem>,
    pub warnings: Vec<String>,
    pub errors: Vec<String>,
}

fn item(kind: &str, name: &str, detail: &str, count: usize) -> ReportItem {
    ReportItem {
        kind: kind.into(),
        name: name.into(),
        detail: detail.into(),
        count,
    }
}

impl ImportReport {
    pub fn new(format: &str, source: &str) -> Self {
        Self {
            format: format.into(),
            source: source.into(),
            ..Self::default()
        }
    }

    /// Counts `n` records read from the file. Call it once per record.
    pub fn seen(&mut self, n: usize) {
        self.records_seen += n;
    }

    pub fn converted(&mut self, kind: &str, name: &str, detail: &str) {
        self.converted.push(item(kind, name, detail, 1));
    }

    pub fn ignored(&mut self, kind: &str, name: &str, reason: &str, count: usize) {
        self.ignored.push(item(kind, name, reason, count));
    }

    pub fn unsupported(&mut self, kind: &str, name: &str, reason: &str, count: usize) {
        self.unsupported.push(item(kind, name, reason, count));
    }

    pub fn warn(&mut self, m: impl Into<String>) {
        self.warnings.push(m.into());
    }

    pub fn error(&mut self, m: impl Into<String>) {
        self.errors.push(m.into());
    }

    pub fn accounted(&self) -> usize {
        self.converted
            .iter()
            .chain(&self.ignored)
            .chain(&self.unsupported)
            .map(|i| i.count)
            .sum()
    }

    pub fn has_errors(&self) -> bool {
        !self.errors.is_empty()
    }

    /// Verifies the "nothing disappears" rule: every record seen is accounted for. A mismatch is
    /// recorded as an error rather than hidden.
    pub fn finish(mut self) -> Self {
        let (seen, acc) = (self.records_seen, self.accounted());
        if acc != seen {
            self.errors.push(format!(
                "internal: the importer read {seen} record(s) but accounted for {acc} (converted + ignored + unsupported); information may have been dropped"
            ));
        }
        self
    }

    pub fn to_json(&self) -> Value {
        let n = |v: &[ReportItem]| v.iter().map(|i| i.count).sum::<usize>();
        json!({
            "format": self.format, "source": self.source, "records_seen": self.records_seen,
            "summary": {"converted": n(&self.converted), "ignored": n(&self.ignored), "unsupported": n(&self.unsupported), "warnings": self.warnings.len(), "errors": self.errors.len()},
            "converted": self.converted, "ignored": self.ignored, "unsupported": self.unsupported,
            "warnings": self.warnings, "errors": self.errors,
        })
    }

    /// Human-readable report.
    pub fn text(&self) -> String {
        let n = |v: &[ReportItem]| v.iter().map(|i| i.count).sum::<usize>();
        let mut o = format!(
            "Import report — {} ({})\n  {} record(s) read: {} converted, {} ignored, {} unsupported; {} warning(s), {} error(s)\n",
            self.source, self.format, self.records_seen, n(&self.converted), n(&self.ignored), n(&self.unsupported), self.warnings.len(), self.errors.len()
        );
        let mut section = |title: &str, v: &[ReportItem]| {
            if v.is_empty() {
                return;
            }
            o.push_str(&format!("\n{title}:\n"));
            for i in v {
                let times = if i.count > 1 {
                    format!(" ×{}", i.count)
                } else {
                    String::new()
                };
                o.push_str(&format!(
                    "  - {} {}{times}{}\n",
                    i.kind,
                    i.name,
                    if i.detail.is_empty() {
                        String::new()
                    } else {
                        format!(": {}", i.detail)
                    }
                ));
            }
        };
        section("Converted", &self.converted);
        section("Ignored", &self.ignored);
        section(
            "Unsupported (information that was NOT imported)",
            &self.unsupported,
        );
        if !self.warnings.is_empty() {
            o.push_str("\nWarnings:\n");
            for w in &self.warnings {
                o.push_str(&format!("  - {w}\n"));
            }
        }
        if !self.errors.is_empty() {
            o.push_str("\nErrors:\n");
            for e in &self.errors {
                o.push_str(&format!("  - {e}\n"));
            }
        }
        o
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unaccounted_records_become_errors() {
        let mut r = ImportReport::new("t", "f");
        r.seen(3);
        r.converted("x", "a", "");
        r.ignored("c", "comments", "not data", 1);
        let r = r.finish();
        assert!(
            r.has_errors()
                && r.errors[0].contains("read 3")
                && r.errors[0].contains("accounted for 2")
        );
        let mut ok = ImportReport::new("t", "f");
        ok.seen(2);
        ok.converted("x", "a", "");
        ok.unsupported("d", ".tran", "analysis", 1);
        let ok = ok.finish();
        assert!(!ok.has_errors());
        assert_eq!(ok.to_json()["summary"]["unsupported"], 1);
        assert!(ok.text().contains("NOT imported"));
    }
}
