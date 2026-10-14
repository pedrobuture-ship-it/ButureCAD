//! DRC in the desktop app: running the check on the open board and jumping to violations.
//!
//! The rules live in `buturecad_rules::drc` and work on the real board geometry; this module only
//! stores the last report, turns it into UI data, and selects/centres the offending objects.

use super::*;
use buturecad_eda_core::model::PcbObject;
use buturecad_geometry::Circle;
use buturecad_pcb::layer;
use buturecad_renderer::{Rgba, Shape};
use buturecad_rules::{run_drc, DrcReport, DrcSeverity, DrcViolation};

/// Violations sent to the UI at most.
const MAX_LISTED: usize = 2000;

#[derive(Default)]
pub(crate) struct DrcState {
    report: Option<DrcReport>,
    revision: u64,
    /// Ring drawn on the violation the user jumped to; cleared by the next click.
    marker: Option<Point>,
}

impl Workspace {
    /// Runs DRC on the open board and stores the report.
    pub fn drc_run(&mut self) -> Result<(), String> {
        let open = self.open.as_ref().ok_or("no project is open")?;
        let revision = open.engine.revision();
        let started = std::time::Instant::now();
        let report = run_drc(open.engine.project().pcb());
        let (errors, warnings) = (report.errors(), report.warnings());
        self.drc.report = Some(report);
        self.drc.revision = revision;
        self.drc.marker = None;
        self.log(
            if errors > 0 {
                MessageLevel::Warn
            } else {
                MessageLevel::Info
            },
            format!(
                "DRC: {errors} error(s), {warnings} warning(s) in {} ms",
                started.elapsed().as_millis()
            ),
        );
        Ok(())
    }

    pub(crate) fn drc_info(&self) -> Option<DrcInfo> {
        let report = self.drc.report.as_ref()?;
        let open = self.open.as_ref()?;
        let violations: Vec<DrcViolationInfo> = report
            .violations
            .iter()
            .enumerate()
            .take(MAX_LISTED)
            .map(|(i, v)| DrcViolationInfo {
                index: i as u32,
                rule: v.rule.code().into(),
                title: v.rule.title().into(),
                severity: match v.severity {
                    DrcSeverity::Error => "error",
                    DrcSeverity::Warning => "warning",
                }
                .into(),
                message: v.message.clone(),
                layer: v.layer.map(layer::name),
            })
            .collect();
        Some(DrcInfo {
            stale: self.drc.revision != open.engine.revision(),
            errors: report.errors() as u32,
            warnings: report.warnings() as u32,
            truncated: report.violations.len().saturating_sub(violations.len()) as u32,
            violations,
        })
    }

    /// Switches to the PCB, selects the offending objects and centres the view on the problem.
    pub fn drc_focus(&mut self, index: usize) -> Result<(), String> {
        let v: DrcViolation = self
            .drc
            .report
            .as_ref()
            .ok_or("run DRC first")?
            .violations
            .get(index)
            .ok_or("that violation no longer exists; run DRC again")?
            .clone();
        let doc = self.pcb_doc().ok_or("no project is open")?;
        let live: BTreeSet<Id> = v
            .objects
            .iter()
            .copied()
            .filter(|id| {
                matches!(
                    doc.object(*id),
                    Some(PcbObject::Track(_) | PcbObject::Via(_) | PcbObject::Footprint(_))
                )
            })
            .collect();
        if live.is_empty() && !v.objects.is_empty() {
            return Err("the objects of this violation no longer exist; run DRC again".into());
        }
        self.set_mode(Mode::Pcb);
        self.ensure_scene();
        self.selection.insert(Mode::Pcb, live);
        self.drc.marker = Some(v.at);
        let min_scale = 8.0 / DEFAULT_GRID_NM as f64;
        let view = self.view_mut();
        if view.scale < min_scale {
            view.scale = min_scale;
        }
        view.center = (v.at.x as f64, v.at.y as f64);
        self.fitted.insert(Mode::Pcb);
        Ok(())
    }

    pub(crate) fn drc_marker_shapes(&self, colour: Rgba) -> Vec<(Shape, Rgba)> {
        match self.drc.marker {
            Some(at) if self.mode == Mode::Pcb => vec![(
                Shape::Circle {
                    circle: Circle {
                        center: at,
                        radius: 1_500_000,
                    },
                    filled: false,
                    stroke: 200_000,
                },
                colour,
            )],
            _ => Vec::new(),
        }
    }

    pub(crate) fn drc_clear_marker(&mut self) {
        self.drc.marker = None;
    }
}
