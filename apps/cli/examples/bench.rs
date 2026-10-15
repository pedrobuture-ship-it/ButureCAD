//! Headless benchmark of the Core on synthetic boards of 100 / 500 / 2000 / 5000 components.
//!
//!     cargo run --release -p buturecad-cli --example bench -- <components> [--json]
//!
//! One process per size (so peak memory is that size's). Prints one JSON object per phase:
//! wall time, CPU time (user+system of this process) and resident memory. `benchmarks/run.sh`
//! runs every size and writes `benchmarks/results/`.
#![allow(clippy::unwrap_used, clippy::print_stdout, clippy::print_stderr)]

use buturecad_eda_core::model::{Id, PcbChange, PcbObject};
use buturecad_eda_core::ratsnest::RatsnestEngine;
use buturecad_eda_core::{CommandEngine, Project};
use buturecad_geometry::Point;
use buturecad_io::ProjectSession;
use buturecad_scripting::{compile, resolve_footprint, CompileOptions};
use std::time::{Duration, Instant};

#[path = "bench_common/mod.rs"]
mod common;
use common::{cpu_s, proc_kb, script_explicit};

struct Bench {
    size: usize,
}

impl Bench {
    /// Runs `f` `reps` times; reports the median wall time and the CPU time of the whole loop / reps.
    fn phase<T>(&self, name: &str, reps: usize, mut f: impl FnMut() -> T) -> T {
        let mut times: Vec<Duration> = Vec::new();
        let cpu0 = cpu_s();
        let mut last = None;
        for _ in 0..reps {
            let t = Instant::now();
            last = Some(f());
            times.push(t.elapsed());
        }
        times.sort();
        let med = times[times.len() / 2];
        let cpu = (cpu_s() - cpu0) / reps as f64;
        println!(
            "{}",
            serde_json::json!({
                "size": self.size, "phase": name, "reps": reps,
                "ms": (med.as_secs_f64() * 1e5).round() / 100.0,
                "cpu_ms": (cpu * 1e5).round() / 100.0,
                "rss_mb": (proc_kb("VmRSS:") / 10.24).round() / 100.0,
                "peak_mb": (proc_kb("VmHWM:") / 10.24).round() / 100.0,
            })
        );
        last.unwrap()
    }
}

fn main() {
    let size: usize = std::env::args()
        .nth(1)
        .and_then(|a| a.parse().ok())
        .unwrap_or(100);
    let b = Bench { size };
    let src = script_explicit(size);
    let reps = if size <= 500 { 3 } else { 1 };

    let out = b.phase("script compile (lex+parse+elaborate)", reps, || {
        compile(&src, &CompileOptions::default())
    });
    assert!(
        !out.has_errors(),
        "{:?}",
        out.diagnostics.iter().take(3).collect::<Vec<_>>()
    );
    let design = out.design.unwrap();

    let engine = b.phase("build project (apply IR + PCB sync + zone fill)", 1, || {
        let mut e = CommandEngine::new(Project::new("bench").unwrap());
        buturecad_eda_core::ir_apply::apply_design(&mut e, &design, &|n| resolve_footprint(n))
            .unwrap();
        e
    });
    let project = engine.project().clone();
    // BUTURECAD_BENCH_DUMP=<file> writes the fingerprint of the built project, to prove that an
    // optimisation did not change the result.
    if let Some(f) = std::env::var_os("BUTURECAD_BENCH_DUMP") {
        let _ = std::fs::write(f, buturecad_agent_api::fingerprint(&project));
    }
    let pcb = project.pcb();
    eprintln!(
        "{} components, {} nets, {} tracks, {} vias, {} zones",
        pcb.placed.len(),
        pcb.nets.len(),
        pcb.tracks.len(),
        pcb.vias.len(),
        pcb.zones.len()
    );

    let dir = std::env::temp_dir().join(format!("buturecad-bench-{}-{size}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    let session = b.phase("save project (atomic, with backup)", reps, || {
        let (s, _) = match ProjectSession::create(&dir, "bench") {
            Ok(x) => x,
            Err(_) => (ProjectSession::open(&dir).unwrap().session, project.clone()),
        };
        s.save(&project).unwrap();
        s
    });
    fn dir_bytes(d: &std::path::Path) -> u64 {
        std::fs::read_dir(d)
            .into_iter()
            .flatten()
            .flatten()
            .map(|e| match e.metadata() {
                Ok(m) if m.is_dir() => dir_bytes(&e.path()),
                Ok(m) => m.len(),
                Err(_) => 0,
            })
            .sum()
    }
    let bytes = dir_bytes(&dir);
    eprintln!("project files: {:.2} MB", bytes as f64 / 1e6);
    drop(session);
    let loaded = b.phase("load project (parse + validate)", reps, || {
        ProjectSession::open(&dir).unwrap()
    });
    assert_eq!(loaded.project, project);
    let _ = std::fs::remove_dir_all(&dir);

    let conn = b.phase("schematic connectivity", reps, || {
        buturecad_schematic::Connectivity::build(project.schematic())
    });
    b.phase("ERC", reps, || {
        buturecad_rules::run_erc(project.schematic(), &conn, project.schematic().waivers())
    });
    let drc = b.phase("DRC", 1, || buturecad_rules::run_drc(pcb));
    eprintln!("DRC: {} errors, {} warnings", drc.errors(), drc.warnings());

    let mut rats = b.phase("ratsnest (full)", reps, || RatsnestEngine::new(pcb));
    let mut doc = pcb.clone();
    let first = *doc.placed.keys().next().unwrap();
    b.phase("ratsnest (move one footprint, incremental)", 5, || {
        let before = doc.object(first).unwrap();
        let PcbObject::Footprint(mut f) = before.clone() else {
            return;
        };
        f.placement.position = Point::new(f.placement.position.x + 1000, f.placement.position.y);
        let ch = PcbChange::modify(first, before, PcbObject::Footprint(f));
        doc.apply_changes(std::slice::from_ref(&ch)).unwrap();
        let nets = buturecad_eda_core::ratsnest::affected_nets(std::slice::from_ref(&ch));
        rats.update(&doc, &nets);
    });

    b.phase("zone refill (all zones)", 1, || {
        buturecad_eda_core::zonefill::fill_changes(pcb, None, true)
    });

    // Routing: walk-around latency on this board.
    let net = pcb.nets.keys().next().copied();
    let params = buturecad_router::RouteParams::new(net, buturecad_pcb::layer::F_CU, 250_000);
    let set = b.phase("router obstacle collection", reps, || {
        buturecad_router::ObstacleSet::collect(pcb, params)
    });
    let (mut ok, mut seed) = (0, 0x2545_F491_4F6C_DD1Du64);
    b.phase("router walk-around (20 routes)", 1, || {
        for _ in 0..20 {
            seed ^= seed << 13;
            seed ^= seed >> 7;
            seed ^= seed << 17;
            let x = 3_000_000 + (seed % 150_000) as i64 * 1000;
            seed ^= seed << 13;
            seed ^= seed >> 7;
            seed ^= seed << 17;
            let y = 2_000_000 + (seed % 50_000) as i64 * 1000;
            if buturecad_router::walk_around(
                &set,
                Point::new(x, y),
                Point::new(x + 12_000_000, y + 3_000_000),
            )
            .is_ok()
            {
                ok += 1;
            }
        }
    });
    eprintln!("routes found: {ok}/20");

    b.phase(
        "manufacturing (preflight + Gerber + drill + BOM + zip)",
        1,
        || {
            let input = buturecad_manufacturing::board::build_fab_input(pcb, "bench");
            let g = buturecad_manufacturing::generate(&input).unwrap();
            if g.report.has_errors() {
                Vec::new()
            } else {
                g.zip().unwrap()
            }
        },
    );
    let _ = Id(0);
}
