//! GUI-side benchmark: rendering, zoom, pan and selection latency on synthetic boards.
//!
//!     cargo run --release -p buturecad-desktop --example bench_ui -- <components>
//!
//! Drives the same `Workspace` the app uses (GPU renderer, scene building, picking) without a
//! window, at 1280×800. One JSON object per phase: median / p95 latency, frames per second (the
//! frame includes the pixel read-back the UI needs), CPU time and memory.
#![allow(clippy::unwrap_used, clippy::print_stdout, clippy::print_stderr)]

#[path = "../../../cli/examples/bench_common/mod.rs"]
mod common;

use buturecad_desktop_lib::ui_types::{Mode, PointerEvent, PointerKind};
use buturecad_desktop_lib::workspace::Workspace;
use buturecad_scripting::{compile, resolve_footprint, CompileOptions};
use common::{cpu_s, proc_kb, script_explicit};
use std::time::{Duration, Instant};

fn report(size: usize, phase: &str, mut times: Vec<Duration>, cpu: f64) {
    times.sort();
    let med = times[times.len() / 2].as_secs_f64() * 1e3;
    let p95 = times[(times.len() * 95 / 100).min(times.len() - 1)].as_secs_f64() * 1e3;
    let total: f64 = times.iter().map(Duration::as_secs_f64).sum();
    println!(
        "{}",
        serde_json::json!({
            "size": size, "phase": phase, "frames": times.len(),
            "ms": (med * 100.0).round() / 100.0, "p95_ms": (p95 * 100.0).round() / 100.0,
            "fps": (times.len() as f64 / total.max(1e-9) * 10.0).round() / 10.0,
            "cpu_ms": (cpu / times.len() as f64 * 1e5).round() / 100.0,
            "rss_mb": (proc_kb("VmRSS:") / 10.24).round() / 100.0, "peak_mb": (proc_kb("VmHWM:") / 10.24).round() / 100.0,
        })
    );
}

fn ev(kind: PointerKind, x: f64, y: f64, button: i32, delta: f64) -> PointerEvent {
    PointerEvent {
        kind,
        x,
        y,
        button,
        shift: false,
        ctrl: false,
        alt: false,
        delta,
    }
}

fn run<F: FnMut()>(size: usize, phase: &str, n: usize, mut f: F) {
    let (cpu0, mut times) = (cpu_s(), Vec::new());
    for _ in 0..n {
        let t = Instant::now();
        f();
        times.push(t.elapsed());
    }
    report(size, phase, times, cpu_s() - cpu0);
}

fn main() {
    let size: usize = std::env::args()
        .nth(1)
        .and_then(|a| a.parse().ok())
        .unwrap_or(100);
    let (w, h) = (1280u32, 800u32);
    let src = script_explicit(size);
    let out = compile(&src, &CompileOptions::default());
    assert!(!out.has_errors());
    let design = out.design.unwrap();

    let dir = std::env::temp_dir().join(format!("buturecad-benchui-{}-{size}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    let t = Instant::now();
    let mut ws = Workspace::new(None);
    report(size, "startup (Workspace::new)", vec![t.elapsed()], 0.0);
    ws.new_project(&dir, "bench").unwrap();
    {
        let e = &mut ws.open.as_mut().unwrap().engine;
        buturecad_eda_core::ir_apply::apply_design(e, &design, &|n| resolve_footprint(n)).unwrap();
    }
    ws.set_canvas_size(f64::from(w), f64::from(h));

    for (mode, name) in [(Mode::Schematic, "schematic"), (Mode::Pcb, "PCB")] {
        ws.set_mode(mode);
        // First frame: builds the scene (and creates the GPU renderer the first time).
        let cpu0 = cpu_s();
        let t = Instant::now();
        ws.fit_view();
        ws.render(w, h).unwrap();
        report(
            size,
            &format!(
                "{name}: first frame (scene build{})",
                if mode == Mode::Schematic {
                    " + GPU init"
                } else {
                    ""
                }
            ),
            vec![t.elapsed()],
            cpu_s() - cpu0,
        );
        run(
            size,
            &format!("{name}: steady frame (cached scene)"),
            30,
            || {
                ws.render(w, h).unwrap();
            },
        );
        // Zoom in and out by one wheel notch around the centre: the whole scene stays visible, so
        // every frame draws the full design (zooming ever deeper would hide most of it).
        let (mut sign, mut rebuilt, mut tess) = (1.0, 0usize, 0.0f64);
        run(
            size,
            &format!("{name}: zoom (wheel ± + frame)"),
            40,
            || {
                ws.pointer(ev(PointerKind::Wheel, 640.0, 400.0, 0, -120.0 * sign));
                sign = -sign;
                ws.render(w, h).unwrap();
                if let Some(st) = ws.render_stats() {
                    if st.mesh_rebuilt {
                        rebuilt += 1;
                        tess += f64::from(st.tessellate_ms);
                    }
                }
            },
        );
        println!(
            "{}",
            serde_json::json!({"size": size, "phase": format!("{name}: zoom frames that re-tessellated the scene"), "frames": 40, "ms": 0.0, "rebuilt": rebuilt, "avg_tessellate_ms": (tess / rebuilt.max(1) as f64 * 100.0).round() / 100.0, "objects": ws.render_stats().map(|s| s.objects), "vertices": ws.render_stats().map(|s| s.vertices), "triangles": ws.render_stats().map(|s| s.triangles)})
        );
        ws.fit_view();
        ws.fit_view();
        let (mut x, mut dir_x) = (640.0, 1.0);
        run(size, &format!("{name}: pan (drag + frame)"), 30, || {
            ws.pointer(ev(PointerKind::Down, x, 400.0, 1, 0.0));
            x += 25.0 * dir_x;
            if !(300.0..=980.0).contains(&x) {
                dir_x = -dir_x;
            }
            ws.pointer(ev(PointerKind::Move, x, 400.0, 1, 0.0));
            ws.pointer(ev(PointerKind::Up, x, 400.0, 1, 0.0));
            ws.render(w, h).unwrap();
        });
        ws.fit_view();
        let mut k = 0.0;
        run(size, &format!("{name}: click selection (pick)"), 40, || {
            k += 1.0;
            let (px, py) = (100.0 + (k * 37.0) % 1000.0, 100.0 + (k * 61.0) % 600.0);
            ws.pointer(ev(PointerKind::Down, px, py, 0, 0.0));
            ws.pointer(ev(PointerKind::Up, px, py, 0, 0.0));
        });
        run(
            size,
            &format!("{name}: box selection of the whole view"),
            5,
            || {
                ws.pointer(ev(PointerKind::Down, 5.0, 5.0, 0, 0.0));
                ws.pointer(ev(PointerKind::Move, 1270.0, 790.0, 0, 0.0));
                ws.pointer(ev(PointerKind::Up, 1270.0, 790.0, 0, 0.0));
            },
        );
    }
    for m in ws
        .snapshot()
        .messages
        .iter()
        .filter(|m| m.text.contains("GPU"))
    {
        eprintln!("{}", m.text);
    }
    let _ = std::fs::remove_dir_all(&dir);
}
