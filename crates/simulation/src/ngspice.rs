//! ngspice as an **external process**.
//!
//! ButureCAD does not link or bundle ngspice: it runs the user's installed `ngspice` binary in
//! batch mode and reads the ASCII raw file it writes. The process boundary keeps ngspice's
//! licence separate from ours and works fully offline. If `ngspice` is not on `PATH` this backend
//! reports itself unavailable.
//!
//! Verified against ngspice 47 by `tests/ngspice_vs_builtin.rs` (skipped when ngspice is absent);
//! the raw-file parser is also covered by fixtures.

use crate::circuit::{Analysis, Circuit};
use crate::result::{Series, SimResult};
use crate::{Backend, SimError};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

const TIMEOUT: Duration = Duration::from_secs(60);

pub struct NgspiceBackend {
    exe: Option<PathBuf>,
}

fn find_on_path(name: &str) -> Option<PathBuf> {
    let path = std::env::var_os("PATH")?;
    std::env::split_paths(&path)
        .map(|d| d.join(name))
        .find(|p| p.is_file())
}

impl NgspiceBackend {
    pub fn detect() -> Self {
        Self {
            exe: find_on_path("ngspice"),
        }
    }

    pub fn with_executable(exe: impl Into<PathBuf>) -> Self {
        Self {
            exe: Some(exe.into()),
        }
    }

    pub fn executable(&self) -> Option<&Path> {
        self.exe.as_deref()
    }
}

impl Backend for NgspiceBackend {
    fn id(&self) -> &'static str {
        "ngspice"
    }
    fn name(&self) -> &'static str {
        "ngspice (external process)"
    }
    fn available(&self) -> bool {
        self.exe.is_some()
    }

    fn run(&self, c: &Circuit, analysis: &Analysis) -> Result<SimResult, SimError> {
        c.validate()?;
        analysis.validate()?;
        let exe = self.exe.as_ref().ok_or(SimError::Unavailable("ngspice"))?;
        let guard = TempDirGuard(tempdir()?);
        let dir = guard.0.clone();
        let net = dir.join("circuit.cir");
        let raw = dir.join("out.raw");
        let mut text = c.to_spice(analysis);
        // Run the analysis from the netlist and write every vector as ASCII.
        text = text.replace(
            ".end\n",
            &format!(
                ".control\nset filetype=ascii\nrun\nwrite {} all\n.endc\n.end\n",
                raw.display()
            ),
        );
        std::fs::write(&net, text).map_err(|e| SimError::Failed(e.to_string()))?;
        // Output goes to files, not pipes: a chatty ngspice cannot fill a pipe and block while
        // we wait for it to exit.
        let log_path = dir.join("ngspice.log");
        let log_file =
            std::fs::File::create(&log_path).map_err(|e| SimError::Failed(e.to_string()))?;
        let log_err = log_file
            .try_clone()
            .map_err(|e| SimError::Failed(e.to_string()))?;
        let mut child = Command::new(exe)
            .arg("-b")
            .arg(&net)
            .current_dir(&dir)
            .stdin(Stdio::null())
            .stdout(Stdio::from(log_file))
            .stderr(Stdio::from(log_err))
            .spawn()
            .map_err(|e| SimError::Failed(format!("cannot start ngspice: {e}")))?;
        let started = Instant::now();
        let status = loop {
            match child.try_wait() {
                Ok(Some(s)) => break s,
                Ok(None) if started.elapsed() > TIMEOUT => {
                    let _ = child.kill();
                    let _ = child.wait();
                    return Err(SimError::Failed(
                        "ngspice did not finish within 60 s and was stopped".into(),
                    ));
                }
                Ok(None) => std::thread::sleep(Duration::from_millis(20)),
                Err(e) => {
                    let _ = child.kill();
                    let _ = child.wait();
                    return Err(SimError::Failed(e.to_string()));
                }
            }
        };
        let log =
            String::from_utf8_lossy(&std::fs::read(&log_path).unwrap_or_default()).into_owned();
        let data = std::fs::read(&raw);
        let data = data.map_err(|_| {
            let tail: String = log
                .lines()
                .rev()
                .take(8)
                .collect::<Vec<_>>()
                .into_iter()
                .rev()
                .collect::<Vec<_>>()
                .join("\n");
            SimError::Failed(format!("ngspice produced no result ({status}):\n{tail}"))
        })?;
        let mut r = parse_raw(&String::from_utf8_lossy(&data), analysis)?;
        r.backend = self.name().into();
        Ok(r)
    }
}

/// Removes the temporary folder on every exit path (success, error, timeout).
struct TempDirGuard(PathBuf);

impl Drop for TempDirGuard {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

fn tempdir() -> Result<PathBuf, SimError> {
    let base = std::env::temp_dir().join(format!(
        "buturecad-ngspice-{}-{}",
        std::process::id(),
        Instant::now().elapsed().as_nanos()
    ));
    // Unique enough per call: add a counter.
    static N: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
    let dir = base.with_extension(
        N.fetch_add(1, std::sync::atomic::Ordering::Relaxed)
            .to_string(),
    );
    std::fs::create_dir_all(&dir).map_err(|e| SimError::Failed(e.to_string()))?;
    Ok(dir)
}

/// Parses an ngspice ASCII raw file into a [`SimResult`] for `analysis`.
pub fn parse_raw(text: &str, analysis: &Analysis) -> Result<SimResult, SimError> {
    let bad = |m: &str| SimError::Failed(format!("unreadable ngspice result: {m}"));
    let mut lines = text.lines();
    let (mut nvars, mut npoints, mut complex) = (0usize, 0usize, false);
    let mut vars: Vec<(String, String)> = Vec::new();
    let mut in_vars = false;
    let mut in_values = false;
    let mut flat: Vec<(f64, f64)> = Vec::new();
    for l in &mut lines {
        if in_values {
            let t = l.trim();
            if t.is_empty() {
                continue;
            }
            // First number of a point is preceded by the point index.
            let val = t
                .split_whitespace()
                .last()
                .ok_or_else(|| bad("empty value"))?;
            let (re, im) = match val.split_once(',') {
                Some((a, b)) => (
                    a.parse::<f64>().map_err(|_| bad("bad number"))?,
                    b.parse::<f64>().map_err(|_| bad("bad number"))?,
                ),
                None => (val.parse::<f64>().map_err(|_| bad("bad number"))?, 0.0),
            };
            flat.push((re, im));
            continue;
        }
        if in_vars {
            if l.trim_start().starts_with("Values:") {
                in_values = true;
                in_vars = false;
                continue;
            }
            let parts: Vec<&str> = l.split_whitespace().collect();
            if parts.len() >= 3 {
                vars.push((parts[1].to_string(), parts[2].to_string()));
            }
            continue;
        }
        if let Some(v) = l.strip_prefix("No. Variables:") {
            nvars = v.trim().parse().map_err(|_| bad("bad variable count"))?;
        } else if let Some(v) = l.strip_prefix("No. Points:") {
            npoints = v.trim().parse().map_err(|_| bad("bad point count"))?;
        } else if let Some(v) = l.strip_prefix("Flags:") {
            complex = v.contains("complex");
        } else if l.trim_start().starts_with("Variables:") {
            in_vars = true;
        }
    }
    if nvars == 0 || npoints == 0 || vars.len() != nvars || flat.len() != nvars * npoints {
        return Err(bad("the variable/point counts do not match the data"));
    }
    let col = |v: usize| -> Vec<(f64, f64)> { (0..npoints).map(|p| flat[p * nvars + v]).collect() };
    let mut r = SimResult {
        analysis: analysis.name().into(),
        ..SimResult::default()
    };
    let first = if matches!(analysis, Analysis::Op) {
        0
    } else {
        1
    };
    match analysis {
        Analysis::Op => {}
        Analysis::Tran { .. } => {
            r.x = col(0).iter().map(|c| c.0).collect();
            r.x_label = "time".into();
            r.x_unit = "s";
        }
        Analysis::DcSweep { .. } => {
            r.x = col(0).iter().map(|c| c.0).collect();
            r.x_label = vars[0].0.clone();
            r.x_unit = if vars[0].1 == "current" { "A" } else { "V" };
        }
        Analysis::Ac { .. } => {
            r.x = col(0).iter().map(|c| c.0).collect();
            r.x_label = "frequency".into();
            r.x_unit = "Hz";
            r.x_log = true;
        }
    }
    for (v, (name, kind)) in vars.iter().enumerate().skip(first) {
        let unit = if kind == "current" { "A" } else { "V" };
        let values = col(v);
        if matches!(analysis, Analysis::Ac { .. }) && complex {
            if kind == "voltage" {
                r.series.push(Series {
                    name: format!("mag({name})"),
                    unit: "dB",
                    values: values
                        .iter()
                        .map(|(a, b)| 20.0 * a.hypot(*b).max(1e-300).log10())
                        .collect(),
                });
                r.series.push(Series {
                    name: format!("phase({name})"),
                    unit: "deg",
                    values: values
                        .iter()
                        .map(|(a, b)| b.atan2(*a).to_degrees())
                        .collect(),
                });
            }
        } else {
            r.series.push(Series {
                name: name.clone(),
                unit,
                values: values.iter().map(|c| c.0).collect(),
            });
        }
    }
    Ok(r)
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;

    const TRAN: &str = "Title: rc\nDate: x\nPlotname: Transient Analysis\nFlags: real\nNo. Variables: 3\nNo. Points: 2\nVariables:\n\t0\ttime\ttime\n\t1\tv(out)\tvoltage\n\t2\tv1#branch\tcurrent\nValues:\n 0\t0.000000000000000e+00\n\t0.000000000000000e+00\n\t-5.0e-03\n 1\t1.000000000000000e-05\n\t4.9e-02\n\t-4.9e-03\n";

    const AC: &str = "Title: rc\nPlotname: AC Analysis\nFlags: complex\nNo. Variables: 2\nNo. Points: 2\nVariables:\n\t0\tfrequency\tfrequency\n\t1\tv(out)\tvoltage\nValues:\n 0\t1.0e+00,0.0e+00\n\t1.0e+00,0.0e+00\n 1\t1.0e+03,0.0e+00\n\t0.5e+00,-0.5e+00\n";

    #[test]
    fn parses_transient_raw_files() {
        let r = parse_raw(
            TRAN,
            &Analysis::Tran {
                step: 1e-5,
                stop: 1e-5,
            },
        )
        .unwrap();
        assert_eq!(r.x, [0.0, 1e-5]);
        assert_eq!(r.series("v(out)").unwrap().values, [0.0, 0.049]);
        assert_eq!(r.series("v1#branch").unwrap().unit, "A");
    }

    #[test]
    fn parses_ac_raw_files_into_magnitude_and_phase() {
        let r = parse_raw(
            AC,
            &Analysis::Ac {
                points_per_decade: 10,
                start_hz: 1.0,
                stop_hz: 1e3,
            },
        )
        .unwrap();
        let m = r.series("mag(v(out))").unwrap();
        assert!((m.values[0]).abs() < 1e-12);
        assert!((m.values[1] - 20.0 * (0.5f64.hypot(0.5)).log10()).abs() < 1e-9);
        assert!((r.series("phase(v(out))").unwrap().values[1] + 45.0).abs() < 1e-9);
        assert!(r.x_log);
    }

    #[test]
    fn operating_point_and_malformed_files() {
        let op = "Flags: real\nNo. Variables: 2\nNo. Points: 1\nVariables:\n\t0\tv(a)\tvoltage\n\t1\tv1#branch\tcurrent\nValues:\n 0\t2.5\n\t-1e-3\n";
        let r = parse_raw(op, &Analysis::Op).unwrap();
        assert_eq!(r.scalar("v(a)"), Some(2.5));
        assert_eq!(r.scalar("v1#branch"), Some(-1e-3));
        assert!(parse_raw("garbage", &Analysis::Op).is_err());
        assert!(parse_raw(
            &TRAN.replace("No. Points: 2", "No. Points: 3"),
            &Analysis::Op
        )
        .is_err());
        assert!(parse_raw(&TRAN.replace("4.9e-02", "abc"), &Analysis::Op).is_err());
    }

    #[test]
    fn missing_executable_is_reported_not_guessed() {
        let n = NgspiceBackend { exe: None };
        assert!(!n.available());
        let c = Circuit {
            title: "t".into(),
            elements: vec![crate::Element {
                name: "V1".into(),
                kind: crate::Kind::V,
                nodes: vec!["a".into(), "0".into()],
                value: crate::Value::Source(crate::SourceSpec::default()),
            }],
            models: vec![],
        };
        assert_eq!(
            n.run(&c, &Analysis::Op),
            Err(SimError::Unavailable("ngspice"))
        );
    }
}
