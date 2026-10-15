//! ButureScript in the editor: CODE, SPLIT and VISUAL views.
//!
//! The pipeline on every change is `parse → validate → EDA IR → preview`. The preview is a
//! second, private `Workspace` whose project is generated from the script in a temporary folder
//! by the same core code as everything else (`buturecad_scripting::build` /
//! `ir_apply`), so the schematic, PCB and 3D views show exactly what the core builds, with the
//! same renderers. The user's own project is never touched by typing; *Generate project* creates
//! a real project from the script.
//!
//! Direction today: **Code → Visual**. The design for Visual → Code (without overwriting code
//! the user wrote) is in `docs/buturescript/code-visual.md`.

use super::*;
use buturecad_scripting::{build, compile, CompileOptions, Severity};

/// Compile options with the built-in `Std` library available to scripts.
fn standard_options() -> CompileOptions {
    CompileOptions {
        symbols: buturecad_libraries::standard::symbols(),
        footprints: buturecad_libraries::standard::footprints(),
        ..CompileOptions::default()
    }
}
use std::sync::atomic::{AtomicU64, Ordering};

static COUNTER: AtomicU64 = AtomicU64::new(0);

pub(crate) struct ScriptState {
    view: String,
    preview_mode: Mode,
    saved: Option<String>,
    current: Option<String>,
    diagnostics: Vec<CodeDiagnostic>,
    summary: String,
    preview: Option<Box<Workspace>>,
    tmp: Vec<PathBuf>,
    generation: u32,
}

impl Default for ScriptState {
    fn default() -> Self {
        Self {
            view: "split".into(),
            preview_mode: Mode::Pcb,
            saved: None,
            current: None,
            diagnostics: Vec::new(),
            summary: String::new(),
            preview: None,
            tmp: Vec::new(),
            generation: 0,
        }
    }
}

impl Drop for ScriptState {
    fn drop(&mut self) {
        for t in &self.tmp {
            let _ = std::fs::remove_dir_all(t);
        }
    }
}

pub const TEMPLATE: &str = r#"// ButureScript — edit and watch the preview update.
project "blinky"

board { outline = rect(0mm, 0mm, 40mm, 25mm)  layers = 2 }

param led_count = 3
param pitch = 6mm

for i in 0..led_count {
  component R[i] { value = "330"  footprint = "0603" }
  component LED[i] { value = "red" footprint = "0603" }
  connect R[i].1 -> "+3V3"
  connect R[i].2 -> LED[i].A
  connect LED[i].K -> GND
  place R[i] at (8mm + i * pitch, 8mm)
  place LED[i] at (8mm + i * pitch, 16mm)
}

rule { clearance = 0.2mm  min_track_width = 0.2mm }
"#;

impl Workspace {
    fn script_path(&self) -> Result<PathBuf, String> {
        Ok(self
            .open
            .as_ref()
            .ok_or("open a project to use the code editor")?
            .root
            .join("design.beda"))
    }

    /// The saved `design.beda`, or the template when there is none yet.
    pub fn script_source(&mut self) -> Result<String, String> {
        let p = self.script_path()?;
        let text = match std::fs::read_to_string(&p) {
            Ok(t) => {
                self.script.saved = Some(t.clone());
                t
            }
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
                self.script.saved = None;
                TEMPLATE.to_owned()
            }
            Err(e) => return Err(format!("cannot read design.beda: {e}")),
        };
        self.script_update(&text);
        Ok(text)
    }

    /// `update` (arg = source), `save` (arg = source), `view code|split|visual`,
    /// `preview schematic|pcb|3d`, `fit`, `generate <folder>` (arg = `folder|source`).
    pub fn script_command(&mut self, name: &str, arg: Option<&str>) -> Result<(), String> {
        let a = arg.unwrap_or("");
        match name {
            "update" => {
                self.script_update(a);
                Ok(())
            }
            "save" => {
                let p = self.script_path()?;
                buturecad_io::atomic::atomic_write(&p, a.as_bytes()).map_err(|e| e.to_string())?;
                self.script.saved = Some(a.to_owned());
                self.script_update(a);
                self.log(MessageLevel::Info, format!("Saved {}", p.display()));
                Ok(())
            }
            "view" => {
                if !matches!(a, "code" | "split" | "visual") {
                    return Err("view is code, split or visual".into());
                }
                self.script.view = a.to_owned();
                Ok(())
            }
            "preview" => {
                self.script.preview_mode = match a {
                    "schematic" => Mode::Schematic,
                    "pcb" => Mode::Pcb,
                    "3d" => Mode::Viewer3d,
                    _ => return Err("preview is schematic, pcb or 3d".into()),
                };
                Ok(())
            }
            "fit" => {
                if let Some(p) = self.script.preview.as_mut() {
                    p.fitted.clear();
                }
                Ok(())
            }
            "generate" => {
                let (folder, source) = a.split_once('|').ok_or("expected <folder>|<source>")?;
                self.script_generate(std::path::Path::new(folder), source)
            }
            other => Err(format!("unknown code action: {other}")),
        }
    }

    /// Re-runs the pipeline for `source`. Keeps the last good preview while the script has errors.
    pub(crate) fn script_update(&mut self, source: &str) {
        self.script.current = Some(source.to_owned());
        let out = compile(source, &standard_options());
        self.script.diagnostics = out
            .diagnostics
            .iter()
            .map(|d| CodeDiagnostic {
                severity: if d.severity == Severity::Error {
                    "error"
                } else {
                    "warning"
                }
                .into(),
                code: d.code.into(),
                message: d.message.clone(),
                line: d.span.line,
                col: d.span.col,
                len: d.span.len,
            })
            .collect();
        let Some(design) = out.design else {
            return;
        };
        self.script.generation += 1;
        let dir = std::env::temp_dir().join(format!(
            "buturecad-preview-{}-{}",
            std::process::id(),
            COUNTER.fetch_add(1, Ordering::Relaxed)
        ));
        let mut pw = Box::new(Workspace::new(None));
        pw.set_canvas_size(self.canvas.0, self.canvas.1);
        let built = pw
            .new_project(&dir, "preview")
            .map_err(|e| e.to_string())
            .and_then(|()| {
                let open = pw.open.as_mut().ok_or("preview project missing")?;
                buturecad_eda_core::ir_apply::apply_design(&mut open.engine, &design, &|n| {
                    buturecad_scripting::resolve_footprint(n)
                })
                .map_err(|e| e.to_string())
            });
        match built {
            Ok(report) => {
                if let Some(old) = self.script.preview.take() {
                    pw.views = old.views.clone();
                    pw.fitted = old.fitted.clone();
                    pw.view3d.camera = old.view3d.camera;
                    pw.view3d.framed = old.view3d.framed;
                }
                self.script.summary = format!(
                    "{} components, {} nets, {} tracks, {} vias, {} zones{}",
                    report.components,
                    report.nets,
                    report.tracks,
                    report.vias,
                    report.zones,
                    if report.warnings.is_empty() {
                        String::new()
                    } else {
                        format!(" · {} PCB sync warning(s)", report.warnings.len())
                    }
                );
                for w in report.warnings {
                    self.script.diagnostics.push(CodeDiagnostic {
                        severity: "warning".into(),
                        code: "W-sync".into(),
                        message: w,
                        line: 1,
                        col: 1,
                        len: 1,
                    });
                }
                self.script.preview = Some(pw);
                self.script.tmp.push(dir);
                // Keep only the folder of the preview that is shown.
                while self.script.tmp.len() > 2 {
                    let old = self.script.tmp.remove(0);
                    let _ = std::fs::remove_dir_all(old);
                }
            }
            Err(m) => {
                let _ = std::fs::remove_dir_all(&dir);
                self.script.diagnostics.push(CodeDiagnostic {
                    severity: "error".into(),
                    code: "E-apply".into(),
                    message: m,
                    line: 1,
                    col: 1,
                    len: 1,
                });
            }
        }
    }

    /// Creates a new project from the script (the generated design is built through the Command
    /// Engine and saved) and opens it. The code editor's file `design.beda` is written next to it.
    fn script_generate(&mut self, folder: &std::path::Path, source: &str) -> Result<(), String> {
        let built = build(source, &standard_options(), None).map_err(|e| match e {
            buturecad_scripting::BuildError::Script(d) => {
                format!(
                    "the script has {} error(s); fix them first",
                    d.iter().filter(|x| x.severity == Severity::Error).count()
                )
            }
            other => other.to_string(),
        })?;
        self.new_project(folder, built.design.name.as_str())?;
        let open = self.open.as_mut().ok_or("project missing")?;
        let report =
            buturecad_eda_core::ir_apply::apply_design(&mut open.engine, &built.design, &|n| {
                buturecad_scripting::resolve_footprint(n)
            })
            .map_err(|e| e.to_string())?;
        buturecad_io::atomic::atomic_write(&folder.join("design.beda"), source.as_bytes())
            .map_err(|e| e.to_string())?;
        self.script = ScriptState::default();
        self.built_for = None;
        self.log(
            MessageLevel::Info,
            format!(
                "Generated project “{}” from the script: {} components, {} nets",
                built.design.name, report.components, report.nets
            ),
        );
        Ok(())
    }

    pub(crate) fn code_info(&mut self) -> CodeInfo {
        let dirty = match (&self.script.current, &self.script.saved) {
            (Some(c), Some(s)) => c != s,
            (Some(c), None) => c != TEMPLATE,
            _ => false,
        };
        CodeInfo {
            view: self.script.view.clone(),
            preview: match self.script.preview_mode {
                Mode::Schematic => "schematic",
                Mode::Viewer3d => "3d",
                _ => "pcb",
            }
            .into(),
            has_preview: self.script.preview.is_some(),
            dirty,
            saved_file: self.script.saved.is_some(),
            diagnostics: self.script.diagnostics.clone(),
            summary: self.script.summary.clone(),
            generation: self.script.generation,
            has_project: self.open.is_some(),
        }
    }

    pub(crate) fn render_code(&mut self, w: u32, h: u32) -> Result<(u32, u32, Vec<u8>), String> {
        let mode = self.script.preview_mode;
        let theme = self.theme;
        let canvas = self.canvas;
        let pw = self
            .script
            .preview
            .as_mut()
            .ok_or("the script has no valid preview yet")?;
        pw.theme = theme;
        pw.set_canvas_size(canvas.0, canvas.1);
        if pw.mode != mode {
            pw.set_mode(mode);
        }
        pw.render(w, h)
    }

    /// The preview is read-only: the wheel zooms and any drag pans; nothing else reaches it.
    pub(crate) fn pointer_code(&mut self, mut ev: PointerEvent) -> PointerResult {
        let mode = self.script.preview_mode;
        let hint = self.hint();
        let Some(pw) = self.script.preview.as_mut() else {
            return PointerResult {
                redraw: false,
                cursor_x_nm: 0,
                cursor_y_nm: 0,
                snap: "none".into(),
                hover: None,
                zoom_percent: 100.0,
                selection_changed: false,
                model_changed: false,
                hint,
            };
        };
        if pw.mode != mode {
            pw.set_mode(mode);
        }
        if mode != Mode::Viewer3d
            && matches!(ev.kind, PointerKind::Down | PointerKind::Up)
            && ev.button == 0
        {
            ev.button = 1; // left drag pans; the preview never edits
        }
        let mut r = pw.pointer(ev);
        r.selection_changed = false;
        r.model_changed = false;
        r.hint = hint;
        r
    }
}
