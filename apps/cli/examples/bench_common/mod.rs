//! Shared by the benchmark harnesses: process metrics and the synthetic board generator.
#![allow(dead_code)]

pub fn proc_kb(key: &str) -> f64 {
    std::fs::read_to_string("/proc/self/status")
        .ok()
        .and_then(|s| s.lines().find(|l| l.starts_with(key)).map(str::to_owned))
        .and_then(|l| {
            l.split_whitespace()
                .nth(1)
                .and_then(|v| v.parse::<f64>().ok())
        })
        .unwrap_or(0.0)
}

/// CPU seconds (user + system) used by this process so far.
pub fn cpu_s() -> f64 {
    let s = std::fs::read_to_string("/proc/self/stat").unwrap_or_default();
    let after = s.rsplit(')').next().unwrap_or("");
    let f: Vec<&str> = after.split_whitespace().collect();
    let t = |i: usize| f.get(i).and_then(|v| v.parse::<f64>().ok()).unwrap_or(0.0);
    (t(11) + t(12)) / 100.0
}

/// `n` 0603 resistors in rows of 50, each row chained pad to pad with real tracks, a ground via
/// per row and a ground pour on the bottom layer (explicit nets so tracks, pads and schematic agree).
pub fn script_explicit(n: usize) -> String {
    let cols = 50usize;
    let rows = n.div_ceil(cols);
    let (w, h) = (cols as f64 * 4.0 + 10.0, rows as f64 * 6.0 + 10.0);
    let mut s = format!(
        "project \"bench\"\nboard {{ outline = rect(0mm, 0mm, {w}mm, {h}mm)  layers = 2 }}\nparam n = {n}\nparam cols = {cols}\n"
    );
    s += "for i in 0..n {\n  component R[i] { value = \"10k\" footprint = \"0603\" }\n  place R[i] at (5mm + (i % cols) * 4mm, 5mm + floor(i / cols) * 6mm)\n";
    s += "  if i % cols < cols - 1 && i < n - 1 {\n    net L[i] { R[i].2, R[i + 1].1 }\n  }\n}\n";
    for i in 0..n {
        if i % cols < cols - 1 && i < n - 1 {
            let (x, y) = (5.0 + (i % cols) as f64 * 4.0, 5.0 + (i / cols) as f64 * 6.0);
            s += &format!(
                "track L{i} on F.Cu width 0.25mm path ({:.3}mm, {y}mm) -> ({:.3}mm, {y}mm)\n",
                x + 0.825,
                x + 3.175
            );
        }
    }
    s += &format!(
        "zone GND on B.Cu {{ outline = rect(1mm, 1mm, {}mm, {}mm) clearance = 0.3mm }}\n",
        w - 1.0,
        h - 1.0
    );
    for r in 0..rows.min(2000) {
        s += &format!("via GND at (2mm, {}mm)\n", 5 + r * 6);
    }
    s += "rule { clearance = 0.2mm  min_track_width = 0.15mm }\n";
    s
}
