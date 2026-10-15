//! `buturecad`: the command-line client of the ButureCAD core.
//!
//! It links the same crates as the desktop app (scripting, eda-core, rules, manufacturing, io)
//! and none of the GUI: no Tauri, no window, no renderer. Every command can print a human
//! report or `--json`. Exit codes: `0` success, `1` the design has errors (build/check/ERC/DRC
//! findings, blocked export), `2` usage or I/O problem.

use buturecad_eda_core::Project;
use buturecad_io::ProjectSession;
use buturecad_manufacturing::board::build_fab_input;
use buturecad_manufacturing::tables;
use buturecad_scripting::{build, compile, CompileOptions, Diagnostic, Severity};
use serde_json::{json, Value};
use std::io::Write;
use std::path::{Path, PathBuf};

const HELP: &str = "\
buturecad — ButureCAD command line (same core as the desktop app, no GUI)

USAGE
  buturecad <command> [options]

COMMANDS
  build <script.beda> --out <project-dir> [--name N] [--param k=v]...
        compile a ButureScript file and generate a saved project
  check <script.beda | project-dir>
        script: diagnostics; project: it loads and is consistent
  erc <project-dir>             electrical rules check on the connectivity graph
  drc <project-dir>             design rules check on the board geometry
  export-gerber <project-dir> --out <dir | file.zip>
        DRC + preflight, then Gerber/Excellon/BOM/pick-and-place (production.zip when --out ends in .zip)
  export-bom <project-dir> [--out <file.csv>]
  inspect components|nets|board <project-dir>
  version | help

OPTIONS
  --json       machine-readable output (all commands)
  --quiet      only the exit code and errors

EXIT CODES  0 ok · 1 design errors / findings · 2 usage or I/O error
";

struct Out<'a> {
    out: &'a mut dyn Write,
    err: &'a mut dyn Write,
    json: bool,
    quiet: bool,
}

impl Out<'_> {
    fn line(&mut self, s: impl AsRef<str>) {
        if !self.json && !self.quiet {
            let _ = writeln!(self.out, "{}", s.as_ref());
        }
    }
    fn json(&mut self, v: &Value) {
        if self.json {
            let _ = writeln!(
                self.out,
                "{}",
                serde_json::to_string_pretty(v).unwrap_or_default()
            );
        }
    }
    fn error(&mut self, s: impl AsRef<str>) {
        let _ = writeln!(self.err, "error: {}", s.as_ref());
    }
}

struct Args {
    positional: Vec<String>,
    out: Option<String>,
    name: Option<String>,
    params: Vec<(String, String)>,
    json: bool,
    quiet: bool,
}

fn parse_args(args: &[String]) -> Result<Args, String> {
    let mut a = Args {
        positional: Vec::new(),
        out: None,
        name: None,
        params: Vec::new(),
        json: false,
        quiet: false,
    };
    let mut it = args.iter();
    while let Some(x) = it.next() {
        match x.as_str() {
            "--json" => a.json = true,
            "--quiet" | "-q" => a.quiet = true,
            "--out" | "-o" => a.out = Some(it.next().ok_or("--out needs a path")?.clone()),
            "--name" => a.name = Some(it.next().ok_or("--name needs a value")?.clone()),
            "--param" => {
                let kv = it.next().ok_or("--param needs name=value")?;
                let (k, v) = kv.split_once('=').ok_or("--param needs name=value")?;
                a.params.push((k.to_owned(), v.to_owned()));
            }
            s if s.starts_with("--") => return Err(format!("unknown option {s}")),
            _ => a.positional.push(x.clone()),
        }
    }
    Ok(a)
}

fn load_project(path: &str) -> Result<(ProjectSession, Project), String> {
    let o =
        ProjectSession::open(path).map_err(|e| format!("cannot open the project “{path}”: {e}"))?;
    Ok((o.session, o.project))
}

fn diag_json(d: &Diagnostic) -> Value {
    json!({
        "severity": match d.severity { Severity::Error => "error", Severity::Warning => "warning" },
        "code": d.code, "message": d.message,
        "line": d.span.line, "column": d.span.col, "length": d.span.len,
    })
}

fn print_diags(o: &mut Out, file: &str, diags: &[Diagnostic]) {
    for d in diags {
        o.line(format!("{file}:{d}"));
    }
}

fn mm(nm: i64) -> f64 {
    nm as f64 / 1e6
}

/// Runs the CLI; returns the process exit code.
pub fn run(args: &[String], out: &mut dyn Write, err: &mut dyn Write) -> i32 {
    let Some((cmd, rest)) = args.split_first() else {
        let _ = write!(err, "{HELP}");
        return 2;
    };
    let a = match parse_args(rest) {
        Ok(a) => a,
        Err(e) => {
            let _ = writeln!(err, "error: {e}\n\n{HELP}");
            return 2;
        }
    };
    let mut o = Out {
        out,
        err,
        json: a.json,
        quiet: a.quiet,
    };
    match dispatch(cmd, &a, &mut o) {
        Ok(code) => code,
        Err(msg) => {
            o.error(msg);
            2
        }
    }
}

fn dispatch(cmd: &str, a: &Args, o: &mut Out) -> Result<i32, String> {
    let pos = |n: usize, what: &str| -> Result<&String, String> {
        a.positional
            .get(n)
            .ok_or_else(|| format!("missing {what}; see `buturecad help`"))
    };
    match cmd {
        "help" | "--help" | "-h" => {
            let _ = write!(o.out, "{HELP}");
            Ok(0)
        }
        "version" | "--version" => {
            o.line(format!("buturecad {}", buturecad_eda_core::VERSION));
            o.json(&json!({"version": buturecad_eda_core::VERSION}));
            Ok(0)
        }
        "build" => cmd_build(pos(0, "the script path")?, a, o),
        "check" => {
            let p = pos(0, "a script or project path")?;
            if Path::new(p).is_dir() {
                cmd_check_project(p, o)
            } else {
                cmd_check_script(p, a, o)
            }
        }
        "erc" => cmd_erc(pos(0, "the project folder")?, o),
        "drc" => cmd_drc(pos(0, "the project folder")?, o),
        "export-gerber" => cmd_gerber(pos(0, "the project folder")?, a, o),
        "export-bom" => cmd_bom(pos(0, "the project folder")?, a, o),
        "inspect" => cmd_inspect(
            pos(0, "components, nets or board")?,
            pos(1, "the project folder")?,
            o,
        ),
        other => Err(format!("unknown command “{other}”; see `buturecad help`")),
    }
}

fn read_script(path: &str) -> Result<String, String> {
    std::fs::read_to_string(path).map_err(|e| format!("cannot read “{path}”: {e}"))
}

fn options(a: &Args) -> CompileOptions {
    CompileOptions {
        params: a.params.iter().cloned().collect(),
    }
}

fn cmd_check_script(path: &str, a: &Args, o: &mut Out) -> Result<i32, String> {
    let src = read_script(path)?;
    let out = compile(&src, &options(a));
    print_diags(o, path, &out.diagnostics);
    let errors = out
        .diagnostics
        .iter()
        .filter(|d| d.severity == Severity::Error)
        .count();
    let warnings = out.diagnostics.len() - errors;
    o.line(format!("{path}: {errors} error(s), {warnings} warning(s)"));
    o.json(&json!({
        "file": path, "ok": errors == 0, "errors": errors, "warnings": warnings,
        "diagnostics": out.diagnostics.iter().map(diag_json).collect::<Vec<_>>(),
        "design": out.design.as_ref().map(|d| json!({
            "name": d.name, "components": d.components.len(), "nets": d.nets.len(),
            "tracks": d.tracks.len(), "vias": d.vias.len(), "zones": d.zones.len(),
        })),
    }));
    Ok(i32::from(errors > 0))
}

fn cmd_build(path: &str, a: &Args, o: &mut Out) -> Result<i32, String> {
    let dest = a.out.as_deref().ok_or("build needs --out <project-dir>")?;
    let src = read_script(path)?;
    let built = match build(&src, &options(a), a.name.as_deref()) {
        Ok(b) => b,
        Err(buturecad_scripting::BuildError::Script(d)) => {
            print_diags(o, path, &d);
            o.json(
                &json!({"ok": false, "diagnostics": d.iter().map(diag_json).collect::<Vec<_>>()}),
            );
            o.line(format!("{path}: build failed"));
            return Ok(1);
        }
        Err(e) => {
            o.json(&json!({"ok": false, "error": e.to_string()}));
            o.line(format!("{path}: build failed: {e}"));
            return Ok(1);
        }
    };
    print_diags(o, path, &built.diagnostics);
    let (session, _) = ProjectSession::create(dest, built.engine.project().name())
        .map_err(|e| format!("cannot create “{dest}”: {e}"))?;
    session
        .save(built.engine.project())
        .map_err(|e| format!("cannot save the project: {e}"))?;
    std::fs::copy(path, Path::new(dest).join("design.beda"))
        .map_err(|e| format!("cannot copy the script: {e}"))?;
    o.line(format!(
        "built project “{}” in {dest}: {} components, {} nets, {} tracks, {} vias, {} zones",
        built.design.name,
        built.report.components,
        built.report.nets,
        built.report.tracks,
        built.report.vias,
        built.report.zones
    ));
    o.json(&json!({
        "ok": true, "project": dest, "name": built.design.name,
        "components": built.report.components, "nets": built.report.nets,
        "tracks": built.report.tracks, "vias": built.report.vias, "zones": built.report.zones,
        "warnings": built.diagnostics.iter().map(diag_json).collect::<Vec<_>>(),
        "sync_warnings": built.report.warnings,
    }));
    Ok(0)
}

fn cmd_check_project(path: &str, o: &mut Out) -> Result<i32, String> {
    let (_s, p) = load_project(path)?;
    let comps = p.pcb().placed.len();
    o.line(format!(
        "{path}: project “{}” loads: {} schematic object(s), {comps} footprint(s), {} track(s)",
        p.name(),
        p.schematic().object_count(),
        p.pcb().tracks.len()
    ));
    o.json(&json!({"ok": true, "name": p.name(), "schematic_objects": p.schematic().object_count(), "footprints": comps, "tracks": p.pcb().tracks.len()}));
    Ok(0)
}

fn cmd_erc(path: &str, o: &mut Out) -> Result<i32, String> {
    let (_s, p) = load_project(path)?;
    let sch = p.schematic();
    let conn = buturecad_schematic::Connectivity::build(sch);
    let report = buturecad_rules::run_erc(sch, &conn, sch.waivers());
    let c = report.counts();
    let mut list = Vec::new();
    for v in &report.violations {
        let sev = format!("{:?}", v.severity).to_lowercase();
        o.line(format!(
            "{sev}[{}]: {}{}",
            v.rule.code(),
            v.message,
            v.waived
                .as_ref()
                .map(|w| format!(" (waived: {w})"))
                .unwrap_or_default()
        ));
        list.push(json!({"severity": sev, "rule": v.rule.code(), "message": v.message, "waived": v.waived}));
    }
    o.line(format!(
        "ERC: {} error(s), {} warning(s), {} waived",
        c.errors, c.warnings, c.waived
    ));
    o.json(&json!({"errors": c.errors, "warnings": c.warnings, "waived": c.waived, "violations": list}));
    Ok(i32::from(c.errors > 0))
}

fn cmd_drc(path: &str, o: &mut Out) -> Result<i32, String> {
    let (_s, p) = load_project(path)?;
    let report = buturecad_rules::run_drc(p.pcb());
    let mut list = Vec::new();
    for v in &report.violations {
        let sev = format!("{:?}", v.severity).to_lowercase();
        o.line(format!(
            "{sev}[{}]: {} at ({:.3}, {:.3}) mm",
            v.rule.code(),
            v.message,
            mm(v.at.x),
            mm(v.at.y)
        ));
        list.push(json!({
            "severity": sev, "rule": v.rule.code(), "message": v.message,
            "x_mm": mm(v.at.x), "y_mm": mm(v.at.y), "objects": v.objects.iter().map(|i| i.0).collect::<Vec<_>>(),
            "actual_nm": v.actual, "required_nm": v.required,
        }));
    }
    o.line(format!(
        "DRC: {} error(s), {} warning(s)",
        report.errors(),
        report.warnings()
    ));
    o.json(&json!({"errors": report.errors(), "warnings": report.warnings(), "violations": list}));
    Ok(i32::from(report.errors() > 0))
}

fn cmd_gerber(path: &str, a: &Args, o: &mut Out) -> Result<i32, String> {
    let dest = a
        .out
        .as_deref()
        .ok_or("export-gerber needs --out <dir | file.zip>")?;
    let (_s, p) = load_project(path)?;
    let input = build_fab_input(p.pcb(), p.name());
    let g = buturecad_manufacturing::generate(&input).map_err(|e| e.to_string())?;
    let issues: Vec<Value> = g
        .report
        .issues
        .iter()
        .map(|i| json!({"severity": format!("{:?}", i.severity).to_lowercase(), "code": i.code, "message": i.message}))
        .collect();
    for i in &g.report.issues {
        o.line(format!(
            "{}[{}]: {}",
            format!("{:?}", i.severity).to_lowercase(),
            i.code,
            i.message
        ));
    }
    if g.report.has_errors() {
        o.line("export blocked by the preflight errors above; nothing was written");
        o.json(&json!({"ok": false, "blocked": true, "issues": issues}));
        return Ok(1);
    }
    let written: Vec<String> = if dest.ends_with(".zip") {
        let bytes = g.zip().map_err(|e| e.to_string())?;
        buturecad_io::atomic::atomic_write(Path::new(dest), &bytes).map_err(|e| e.to_string())?;
        g.files.iter().map(|f| f.name.clone()).collect()
    } else {
        std::fs::create_dir_all(dest).map_err(|e| format!("cannot create “{dest}”: {e}"))?;
        for f in &g.files {
            buturecad_io::atomic::atomic_write(&PathBuf::from(dest).join(&f.name), &f.bytes)
                .map_err(|e| e.to_string())?;
        }
        g.files.iter().map(|f| f.name.clone()).collect()
    };
    o.line(format!(
        "wrote {} file(s) to {dest}; this is not fabrication sign-off",
        written.len()
    ));
    o.json(&json!({"ok": true, "destination": dest, "files": written, "issues": issues}));
    Ok(0)
}

fn cmd_bom(path: &str, a: &Args, o: &mut Out) -> Result<i32, String> {
    let (_s, p) = load_project(path)?;
    let csv = tables::bom_csv(&build_fab_input(p.pcb(), p.name()).parts);
    match a.out.as_deref() {
        Some(dest) => {
            buturecad_io::atomic::atomic_write(Path::new(dest), csv.as_bytes())
                .map_err(|e| e.to_string())?;
            o.line(format!("wrote {dest}"));
            o.json(
                &json!({"ok": true, "file": dest, "lines": csv.lines().count().saturating_sub(1)}),
            );
        }
        None if o.json => o.json(&json!({"csv": csv})),
        None => {
            let _ = write!(o.out, "{csv}");
        }
    }
    Ok(0)
}

fn cmd_inspect(what: &str, path: &str, o: &mut Out) -> Result<i32, String> {
    let (_s, p) = load_project(path)?;
    match what {
        "components" => {
            let sch = p.schematic();
            let mut rows = Vec::new();
            for f in p.pcb().placed.values() {
                rows.push((
                    f.reference.clone(),
                    f.value.clone(),
                    f.footprint.clone(),
                    Some((
                        f.placement.position.x,
                        f.placement.position.y,
                        f.placement.rotation,
                        f.placement.flipped,
                    )),
                ));
            }
            let mut sch_rows = Vec::new();
            for sheet in sch.sheets().values() {
                for obj in sheet.objects.values() {
                    if let buturecad_schematic::Object::Component(c) = obj {
                        sch_rows.push((
                            c.reference.clone(),
                            c.value.clone(),
                            c.footprint.clone(),
                            c.lib_id.clone(),
                        ));
                    }
                }
            }
            sch_rows.sort();
            rows.sort_by(|a, b| a.0.cmp(&b.0));
            let mut list = Vec::new();
            for (r, v, fp, sym) in &sch_rows {
                let placed = rows.iter().find(|x| &x.0 == r).and_then(|x| x.3);
                o.line(format!(
                    "{r:<8} {v:<10} {fp:<14} {sym:<14} {}",
                    placed
                        .map(|(x, y, rot, fl)| format!(
                            "({:.3}, {:.3}) mm rot {}° {}",
                            mm(x),
                            mm(y),
                            u32::from(rot) * 90,
                            if fl { "bottom" } else { "top" }
                        ))
                        .unwrap_or_else(|| "not on the board".into())
                ));
                list.push(json!({
                    "reference": r, "value": v, "footprint": fp, "symbol": sym,
                    "placement": placed.map(|(x, y, rot, fl)| json!({"x_mm": mm(x), "y_mm": mm(y), "rotation_deg": u32::from(rot) * 90, "side": if fl { "bottom" } else { "top" }})),
                }));
            }
            o.json(&json!({"components": list}));
        }
        "nets" => {
            let conn = buturecad_schematic::Connectivity::build(p.schematic());
            let mut list = Vec::new();
            for n in conn.nets() {
                let pins: Vec<String> = n
                    .pins
                    .iter()
                    .map(|x| format!("{}.{}", x.reference, x.pin))
                    .collect();
                o.line(format!("{:<16} {}", n.name, pins.join(" ")));
                list.push(json!({"name": n.name, "named": n.named, "pins": pins}));
            }
            o.json(&json!({"nets": list}));
        }
        "board" => {
            let pcb = p.pcb();
            let loops = buturecad_eda_core::outline::outline_loops(pcb);
            let bounds = loops.first().and_then(|l| {
                let first = *l.first()?;
                Some(l.iter().fold((first, first), |(lo, hi), q| {
                    (
                        buturecad_geometry::Point::new(lo.x.min(q.x), lo.y.min(q.y)),
                        buturecad_geometry::Point::new(hi.x.max(q.x), hi.y.max(q.y)),
                    )
                }))
            });
            let size = bounds.map(|(lo, hi)| (mm(hi.x - lo.x), mm(hi.y - lo.y)));
            o.line(format!("copper layers: {}", pcb.stackup.copper_layers));
            o.line(match size {
                Some((w, h)) => format!("outline: {w:.3} x {h:.3} mm"),
                None => "outline: none (no closed Edge.Cuts loop)".into(),
            });
            o.line(format!(
                "footprints {}, nets {}, tracks {}, vias {}, zones {}",
                pcb.placed.len(),
                pcb.nets.len(),
                pcb.tracks.len(),
                pcb.vias.len(),
                pcb.zones.len()
            ));
            o.line(format!(
                "board thickness {:.3} mm",
                mm(pcb.physical.thickness())
            ));
            o.json(&json!({
                "copper_layers": pcb.stackup.copper_layers,
                "outline_mm": size.map(|(w, h)| json!({"width": w, "height": h})),
                "thickness_mm": mm(pcb.physical.thickness()),
                "footprints": pcb.placed.len(), "nets": pcb.nets.len(), "tracks": pcb.tracks.len(),
                "vias": pcb.vias.len(), "zones": pcb.zones.len(),
                "rules": {"min_track_width_mm": mm(pcb.rules.min_track_width), "min_via_diameter_mm": mm(pcb.rules.min_via_diameter)},
            }));
        }
        other => {
            return Err(format!(
                "inspect what? “{other}” (components, nets or board)"
            ))
        }
    }
    Ok(0)
}
