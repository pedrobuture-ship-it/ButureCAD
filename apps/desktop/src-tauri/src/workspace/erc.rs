//! ERC in the desktop app: running the check, jumping to findings, and documented waivers.
//!
//! The check itself lives in `buturecad_rules::erc` and works on the connectivity graph; this
//! module only stores the last report, turns it into UI data, and routes waivers through the
//! Command Engine (so adding or removing one is undoable).

use super::*;
use buturecad_eda_core::commands::SetWaiversCommand;
use buturecad_geometry::Circle;
use buturecad_renderer::{Rgba, Shape};
use buturecad_rules::{run_erc, ErcReport};
use buturecad_schematic::Waiver;

/// Findings sent to the UI at most (a runaway design must not freeze the IPC).
const MAX_LISTED: usize = 2000;

#[derive(Default)]
pub(crate) struct ErcState {
    report: Option<ErcReport>,
    /// Engine revision the report was computed at.
    revision: u64,
    /// Ring drawn around the finding the user jumped to; cleared by the next click.
    marker: Option<Point>,
}

impl Workspace {
    /// Runs ERC on the open project and stores the report.
    pub fn erc_run(&mut self) -> Result<(), String> {
        let open = self.open.as_ref().ok_or("no project is open")?;
        let revision = open.engine.revision();
        let sch = open.engine.project().schematic();
        let report = self
            .connectivity(|c| run_erc(sch, c, sch.waivers()))
            .ok_or("no project is open")?;
        let counts = report.counts();
        self.erc.report = Some(report);
        self.erc.revision = revision;
        self.erc.marker = None;
        self.log(
            if counts.errors > 0 {
                MessageLevel::Warn
            } else {
                MessageLevel::Info
            },
            format!(
                "ERC: {} error(s), {} warning(s), {} info, {} waived",
                counts.errors, counts.warnings, counts.infos, counts.waived
            ),
        );
        Ok(())
    }

    pub(crate) fn erc_info(&self) -> Option<ErcInfo> {
        let report = self.erc.report.as_ref()?;
        let open = self.open.as_ref()?;
        let sch = open.engine.project().schematic();
        let counts = report.counts();
        let root = sch.root();
        let violations: Vec<ViolationInfo> = report
            .violations
            .iter()
            .enumerate()
            .take(MAX_LISTED)
            .map(|(i, v)| ViolationInfo {
                index: i as u32,
                rule: v.rule.code().into(),
                title: v.rule.title().into(),
                severity: v.severity.id().into(),
                message: v.message.clone(),
                sheet: v
                    .sheet
                    .and_then(|s| sch.sheet(s))
                    .map_or_else(String::new, |s| s.name.clone()),
                can_focus: (v.at.is_some() || !v.objects.is_empty())
                    && v.sheet.is_none_or(|s| s == root),
                fingerprint: v.fingerprint.clone(),
                waived: v.waived.clone(),
            })
            .collect();
        Some(ErcInfo {
            stale: self.erc.revision != open.engine.revision(),
            errors: counts.errors as u32,
            warnings: counts.warnings as u32,
            infos: counts.infos as u32,
            waived: counts.waived as u32,
            truncated: report.violations.len().saturating_sub(violations.len()) as u32,
            violations,
        })
    }

    /// Selects the finding's objects in the schematic and centres the view on it.
    pub fn erc_focus(&mut self, index: usize) -> Result<(), String> {
        let report = self.erc.report.as_ref().ok_or("run ERC first")?;
        let v = report
            .violations
            .get(index)
            .ok_or("that finding no longer exists; run ERC again")?
            .clone();
        let open = self.open.as_ref().ok_or("no project is open")?;
        let sch = open.engine.project().schematic();
        if v.sheet.is_some_and(|s| s != sch.root()) {
            let name = v
                .sheet
                .and_then(|s| sch.sheet(s))
                .map_or("another sheet", |s| s.name.as_str());
            return Err(format!(
                "this finding is in “{name}”; only the root sheet can be shown for now"
            ));
        }
        let live: BTreeSet<Id> = v
            .objects
            .iter()
            .copied()
            .filter(|id| sch.object(*id).is_some())
            .collect();
        if live.is_empty() && v.at.is_none() {
            return Err("the objects of this finding no longer exist; run ERC again".into());
        }
        let target = v.at.or_else(|| {
            live.iter()
                .next()
                .and_then(|id| sch.object(*id))
                .map(|(_, o)| sch.object_bounds(o).center())
        });
        self.set_mode(Mode::Schematic);
        self.ensure_scene();
        self.selection.insert(Mode::Schematic, live);
        self.erc.marker = target;
        if let Some(at) = target {
            // Keep the user's zoom unless it is too far out to see the finding.
            let min_scale = 8.0 / DEFAULT_GRID_NM as f64;
            let view = self.view_mut();
            if view.scale < min_scale {
                view.scale = min_scale;
            }
            view.center = (at.x as f64, at.y as f64);
        }
        self.fitted.insert(Mode::Schematic);
        Ok(())
    }

    /// Documents an exception for the finding with `fingerprint`. A reason is required.
    pub fn erc_waive(&mut self, fingerprint: &str, reason: &str) -> Result<(), String> {
        let reason = reason.trim();
        if reason.is_empty() {
            return Err("a waiver needs a documented reason".into());
        }
        let report = self.erc.report.as_ref().ok_or("run ERC first")?;
        let v = report
            .violations
            .iter()
            .find(|v| v.fingerprint == fingerprint)
            .ok_or("that finding no longer exists; run ERC again")?;
        let waiver = Waiver {
            fingerprint: fingerprint.to_owned(),
            rule: v.rule.code().to_owned(),
            subject: v.message.clone(),
            reason: reason.to_owned(),
        };
        let open = self.open.as_mut().ok_or("no project is open")?;
        let before: Vec<Waiver> = open
            .engine
            .project()
            .schematic()
            .waivers()
            .cloned()
            .collect();
        let mut after: Vec<Waiver> = before
            .iter()
            .filter(|w| w.fingerprint != fingerprint)
            .cloned()
            .collect();
        after.push(waiver);
        open.engine
            .execute(SetWaiversCommand::new("Waive ERC finding", before, after))
            .map_err(|e| e.to_string())?;
        self.command_log.push("Waive ERC finding".into());
        self.erc_run()
    }

    /// Removes a waiver; the finding becomes active again.
    pub fn erc_unwaive(&mut self, fingerprint: &str) -> Result<(), String> {
        let open = self.open.as_mut().ok_or("no project is open")?;
        let before: Vec<Waiver> = open
            .engine
            .project()
            .schematic()
            .waivers()
            .cloned()
            .collect();
        let after: Vec<Waiver> = before
            .iter()
            .filter(|w| w.fingerprint != fingerprint)
            .cloned()
            .collect();
        if after.len() == before.len() {
            return Err("there is no waiver for that finding".into());
        }
        open.engine
            .execute(SetWaiversCommand::new("Remove ERC waiver", before, after))
            .map_err(|e| e.to_string())?;
        self.command_log.push("Remove ERC waiver".into());
        self.erc_run()
    }

    /// The ring marking the finding the user jumped to.
    pub(crate) fn erc_marker_shapes(&self, colour: Rgba) -> Vec<(Shape, Rgba)> {
        match self.erc.marker {
            Some(at) if self.mode == Mode::Schematic => vec![(
                Shape::Circle {
                    circle: Circle {
                        center: at,
                        radius: 2_000_000,
                    },
                    filled: false,
                    stroke: 250_000,
                },
                colour,
            )],
            _ => Vec::new(),
        }
    }

    /// Any click or Esc dismisses the marker.
    pub(crate) fn erc_clear_marker(&mut self) {
        self.erc.marker = None;
    }
}
