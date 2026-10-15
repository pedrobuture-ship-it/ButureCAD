//! Runs the real `ngspice` (when installed) and compares it with the built-in engine and with
//! analytic results. Skipped, loudly, when ngspice is not on PATH.
#![allow(clippy::unwrap_used, clippy::panic)]

use buturecad_simulation::*;

fn el(name: &str, kind: Kind, nodes: &[&str], value: Value) -> Element {
    Element {
        name: name.into(),
        kind,
        nodes: nodes.iter().map(|s| s.to_string()).collect(),
        value,
    }
}

fn rc(src: &str) -> Circuit {
    Circuit {
        title: "rc".into(),
        elements: vec![
            el(
                "V1",
                Kind::V,
                &["in", "0"],
                Value::Source(SourceSpec::parse(src).unwrap()),
            ),
            el("R1", Kind::R, &["in", "out"], Value::Number(1000.0)),
            el("C1", Kind::C, &["out", "0"], Value::Number(1e-6)),
        ],
        models: vec![],
    }
}

fn ng() -> Option<NgspiceBackend> {
    let n = NgspiceBackend::detect();
    if !n.available() {
        eprintln!("SKIPPED: ngspice is not installed");
        return None;
    }
    Some(n)
}

#[test]
fn op_ac_and_transient_agree_with_the_builtin_engine() {
    let Some(n) = ng() else { return };
    let b = BuiltinBackend;
    // Operating point.
    let c = rc("5");
    let (a, o) = (
        n.run(&c, &Analysis::Op).unwrap(),
        b.run(&c, &Analysis::Op).unwrap(),
    );
    assert!((a.scalar("v(out)").unwrap() - o.scalar("v(out)").unwrap()).abs() < 1e-6);
    // AC.
    let c = rc("AC 1");
    let an = Analysis::Ac {
        points_per_decade: 10,
        start_hz: 10.0,
        stop_hz: 1e4,
    };
    let (a, o) = (n.run(&c, &an).unwrap(), b.run(&c, &an).unwrap());
    assert_eq!(a.x.len(), o.x.len());
    let (am, om) = (
        a.series("mag(v(out))").unwrap(),
        o.series("mag(v(out))").unwrap(),
    );
    let (ap, op) = (
        a.series("phase(v(out))").unwrap(),
        o.series("phase(v(out))").unwrap(),
    );
    for i in 0..a.x.len() {
        assert!(
            (am.values[i] - om.values[i]).abs() < 1e-3,
            "mag at {}",
            a.x[i]
        );
        assert!(
            (ap.values[i] - op.values[i]).abs() < 1e-2,
            "phase at {}",
            a.x[i]
        );
    }
    // Transient: both follow 5 (1 - e^(-t/RC)).
    let c = rc("PULSE(0 5 0 1n 1n 1 2)");
    let tr = Analysis::Tran {
        step: 1e-5,
        stop: 5e-3,
    };
    let a = n.run(&c, &tr).unwrap();
    let v = a.series("v(out)").unwrap();
    for (t, y) in a.x.iter().zip(&v.values) {
        if *t > 2e-4 {
            let want = 5.0 * (1.0 - (-t / 1e-3).exp());
            assert!((y - want).abs() < 0.03, "t={t}: {y} vs {want}");
        }
    }
}

#[test]
fn a_diode_circuit_runs_in_ngspice_and_is_refused_by_the_builtin_engine() {
    let Some(n) = ng() else { return };
    let c = Circuit {
        title: "diode".into(),
        elements: vec![
            el(
                "V1",
                Kind::V,
                &["in", "0"],
                Value::Source(SourceSpec::parse("5").unwrap()),
            ),
            el("R1", Kind::R, &["in", "a"], Value::Number(1000.0)),
            el("D1", Kind::D, &["a", "0"], Value::Model("D1N4148".into())),
        ],
        models: vec![".model D1N4148 D(IS=2.52e-9 N=1.752 RS=0.568)".into()],
    };
    assert!(matches!(
        BuiltinBackend.run(&c, &Analysis::Op),
        Err(SimError::Unsupported { .. })
    ));
    let r = n.run(&c, &Analysis::Op).unwrap();
    let vd = r.scalar("a").or_else(|| r.scalar("v(a)")).unwrap();
    assert!(vd > 0.5 && vd < 0.9, "diode drop {vd}");
}

#[test]
fn ngspice_errors_are_reported() {
    let Some(n) = ng() else { return };
    let mut c = rc("5");
    c.elements.push(el(
        "D1",
        Kind::D,
        &["out", "0"],
        Value::Model("NOSUCHMODEL".into()),
    ));
    let e = n.run(&c, &Analysis::Op).unwrap_err();
    assert!(matches!(e, SimError::Failed(_)), "{e:?}");
}

/// Release cleanup (stage 49): the temporary folder is removed on success and on error, and a
/// chatty run cannot block on a full output pipe.
#[test]
fn no_temporary_folder_is_left_behind() {
    let Some(n) = ng() else { return };
    let mine = format!("buturecad-ngspice-{}-", std::process::id());
    let left = || {
        std::fs::read_dir(std::env::temp_dir())
            .unwrap()
            .filter_map(Result::ok)
            .filter(|e| e.file_name().to_string_lossy().starts_with(&mine))
            .count()
    };
    n.run(&rc("5"), &Analysis::Op).unwrap();
    let mut bad = rc("5");
    bad.elements.push(el(
        "D1",
        Kind::D,
        &["out", "0"],
        Value::Model("NOSUCHMODEL".into()),
    ));
    assert!(n.run(&bad, &Analysis::Op).is_err());
    // Other tests of this binary run ngspice at the same time: wait for their folders to go.
    let t = std::time::Instant::now();
    while left() > 0 && t.elapsed() < std::time::Duration::from_secs(20) {
        std::thread::sleep(std::time::Duration::from_millis(50));
    }
    assert_eq!(
        left(),
        0,
        "temporary folders were left in the temp directory"
    );
}
