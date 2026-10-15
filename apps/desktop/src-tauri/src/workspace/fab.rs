//! Manufacturing export in the desktop app.
//!
//! The board → `FabInput` bridge lives in `buturecad_manufacturing::board` (shared with the CLI);
//! this module keeps the last preview, writes `production.zip` atomically and exposes the actions.
//! Errors in the preflight block the zip.

use super::*;
use buturecad_manufacturing as fab;
use buturecad_manufacturing::Severity;

/// Last manufacturing preview.
#[derive(Default)]
pub(crate) struct FabState {
    pub(super) generated: Option<(u64, fab::Generated)>,
}

impl Workspace {
    /// DRC + board validation + preflight + generation of every file (nothing is written).
    pub fn fab_preview(&mut self) -> Result<(), String> {
        let open = self.open.as_ref().ok_or("no project is open")?;
        let revision = open.engine.revision();
        let input = buturecad_manufacturing::board::build_fab_input_for_project(
            open.engine.project(),
            open.engine.project().name(),
            &Default::default(),
        );
        let generated = fab::generate(&input).map_err(|e| e.to_string())?;
        let (e, w) = (
            generated.report.count(Severity::Error),
            generated.report.count(Severity::Warning),
        );
        self.fab.generated = Some((revision, generated));
        self.log(
            if e > 0 {
                MessageLevel::Warn
            } else {
                MessageLevel::Info
            },
            format!("Manufacturing preview: {e} error(s), {w} warning(s)"),
        );
        Ok(())
    }

    /// Writes `production.zip` to `path` (refused while the preflight has errors). The preview is
    /// regenerated first so the file always matches the current board.
    pub fn fab_export(&mut self, path: &str) -> Result<(), String> {
        self.fab_preview()?;
        let Some((_, g)) = self.fab.generated.as_ref() else {
            return Err("no preview".into());
        };
        let bytes = g.zip().map_err(|e| e.to_string())?;
        buturecad_io::atomic::atomic_write(std::path::Path::new(path), &bytes)
            .map_err(|e| e.to_string())?;
        let (n, w) = (g.files.len(), g.report.count(Severity::Warning));
        self.log(
            MessageLevel::Info,
            format!("Wrote {path}: {n} file(s), {w} warning(s). This is not fabrication sign-off."),
        );
        Ok(())
    }

    pub(crate) fn fab_info(&self) -> Option<FabInfo> {
        let (rev, g) = self.fab.generated.as_ref()?;
        let open = self.open.as_ref()?;
        Some(FabInfo {
            stale: *rev != open.engine.revision(),
            blocked: g.report.has_errors(),
            errors: g.report.count(Severity::Error) as u32,
            warnings: g.report.count(Severity::Warning) as u32,
            issues: g
                .report
                .issues
                .iter()
                .map(|i| FabIssueInfo {
                    severity: match i.severity {
                        Severity::Error => "error",
                        Severity::Warning => "warning",
                        Severity::Info => "note",
                    }
                    .into(),
                    code: i.code.into(),
                    message: i.message.clone(),
                })
                .collect(),
            files: g
                .preview()
                .into_iter()
                .map(|f| FabFileInfo {
                    name: f.name,
                    bytes: f.bytes as u64,
                    summary: f.summary,
                })
                .collect(),
        })
    }
}
