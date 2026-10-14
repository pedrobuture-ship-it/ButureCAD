//! The built-in engine: modified nodal analysis for linear circuits.
//!
//! Unknowns are the node voltages (ground excluded) plus one branch current per voltage source
//! and per inductor. Every analysis solves complex linear systems with partial pivoting; OP, DC
//! sweep and transient simply keep the imaginary part at zero. Transient uses the trapezoidal
//! rule with a fixed step from the DC operating point at t = 0. A tiny conductance (1e-12 S) ties
//! every node to ground so that capacitor-only nodes have a DC solution, as SPICE does (`gmin`).

use crate::circuit::{Analysis, Circuit, Element, Kind, SourceSpec, Value};
use crate::result::{Series, SimResult};
use crate::{Backend, SimError};
use std::collections::BTreeMap;
use std::f64::consts::PI;

const GMIN: f64 = 1e-12;
const MAX_UNKNOWNS: usize = 600;

#[derive(Clone, Copy, Default, Debug, PartialEq)]
struct C {
    re: f64,
    im: f64,
}

impl C {
    fn new(re: f64, im: f64) -> Self {
        Self { re, im }
    }
    fn abs(self) -> f64 {
        self.re.hypot(self.im)
    }
    fn arg(self) -> f64 {
        self.im.atan2(self.re)
    }
}

impl std::ops::Add for C {
    type Output = C;
    fn add(self, o: C) -> C {
        C::new(self.re + o.re, self.im + o.im)
    }
}
impl std::ops::Sub for C {
    type Output = C;
    fn sub(self, o: C) -> C {
        C::new(self.re - o.re, self.im - o.im)
    }
}
impl std::ops::Mul for C {
    type Output = C;
    fn mul(self, o: C) -> C {
        C::new(
            self.re * o.re - self.im * o.im,
            self.re * o.im + self.im * o.re,
        )
    }
}
impl std::ops::Div for C {
    type Output = C;
    fn div(self, o: C) -> C {
        let d = o.re * o.re + o.im * o.im;
        C::new(
            (self.re * o.re + self.im * o.im) / d,
            (self.im * o.re - self.re * o.im) / d,
        )
    }
}

/// Solves `a x = b`. Err on a (numerically) singular matrix.
fn solve(mut a: Vec<Vec<C>>, mut b: Vec<C>) -> Result<Vec<C>, SimError> {
    let n = b.len();
    let scale = a
        .iter()
        .flatten()
        .map(|c| c.abs())
        .fold(0.0, f64::max)
        .max(1e-30);
    for col in 0..n {
        let piv = (col..n)
            .max_by(|&x, &y| a[x][col].abs().total_cmp(&a[y][col].abs()))
            .unwrap_or(col);
        if a[piv][col].abs() < scale * 1e-14 {
            return Err(SimError::Failed(
                "the circuit equations are singular: a node may be floating, or voltage sources/inductors form a loop".into(),
            ));
        }
        a.swap(col, piv);
        b.swap(col, piv);
        for r in col + 1..n {
            let f = a[r][col] / a[col][col];
            if f.abs() == 0.0 {
                continue;
            }
            let pivot_row = a[col].clone();
            for (cell, p) in a[r].iter_mut().zip(&pivot_row).skip(col) {
                *cell = *cell - f * *p;
            }
            let t = b[col];
            b[r] = b[r] - f * t;
        }
    }
    let mut x = vec![C::default(); n];
    for r in (0..n).rev() {
        let mut s = b[r];
        for k in r + 1..n {
            s = s - a[r][k] * x[k];
        }
        x[r] = s / a[r][r];
    }
    Ok(x)
}

struct Net<'a> {
    nodes: Vec<String>,
    node_ix: BTreeMap<String, usize>,
    /// Branch unknown index (after the nodes) of V sources and inductors, by element position.
    branch: BTreeMap<usize, usize>,
    els: &'a [Element],
    size: usize,
}

impl<'a> Net<'a> {
    fn new(c: &'a Circuit) -> Result<Self, SimError> {
        let mut nodes: Vec<String> = Vec::new();
        for e in &c.elements {
            for n in &e.nodes {
                if n != "0" && !nodes.contains(n) {
                    nodes.push(n.clone());
                }
            }
        }
        let node_ix = nodes
            .iter()
            .enumerate()
            .map(|(i, n)| (n.clone(), i))
            .collect();
        let mut branch = BTreeMap::new();
        let mut next = nodes.len();
        for (i, e) in c.elements.iter().enumerate() {
            match e.kind {
                Kind::V | Kind::L => {
                    branch.insert(i, next);
                    next += 1;
                }
                Kind::R | Kind::C | Kind::I => {}
                other => {
                    return Err(SimError::Unsupported {
                        backend: "built-in engine",
                        reason: format!(
                            "{} is a {} — nonlinear devices and subcircuits need ngspice (install it and select it as the backend)",
                            e.name,
                            match other {
                                Kind::D => "diode",
                                Kind::Q => "bipolar transistor",
                                Kind::M => "MOSFET",
                                _ => "subcircuit",
                            }
                        ),
                    })
                }
            }
        }
        if next > MAX_UNKNOWNS {
            return Err(SimError::Unsupported {
                backend: "built-in engine",
                reason: format!("{next} unknowns exceed the limit of {MAX_UNKNOWNS}"),
            });
        }
        Ok(Self {
            nodes,
            node_ix,
            branch,
            els: &c.elements,
            size: next,
        })
    }

    fn ix(&self, n: &str) -> Option<usize> {
        self.node_ix.get(n).copied()
    }

    fn number(e: &Element) -> Result<f64, SimError> {
        match &e.value {
            Value::Number(v) => Ok(*v),
            _ => Err(SimError::Circuit(format!(
                "{} needs a numeric value",
                e.name
            ))),
        }
    }

    fn source(e: &Element) -> Result<&SourceSpec, SimError> {
        match &e.value {
            Value::Source(s) => Ok(s),
            _ => Err(SimError::Circuit(format!(
                "{} needs a source value",
                e.name
            ))),
        }
    }
}

enum Mode {
    /// DC: capacitors open, inductors short.
    Dc,
    /// Small-signal AC at ω.
    Ac(f64),
    /// One trapezoidal step of size h with the previous state.
    Tran { h: f64, be: bool },
}

struct State {
    v: Vec<f64>,
    cap_i: BTreeMap<usize, f64>,
    ind_i: BTreeMap<usize, f64>,
    ind_v: BTreeMap<usize, f64>,
}

fn stamp(a: &mut [Vec<C>], r: Option<usize>, c: Option<usize>, v: C) {
    if let (Some(r), Some(c)) = (r, c) {
        a[r][c] = a[r][c] + v;
    }
}

/// Builds and solves one system. `src` gives the value of each independent source element.
fn assemble_solve(
    net: &Net<'_>,
    mode: &Mode,
    src: &dyn Fn(usize, &SourceSpec) -> C,
    state: Option<&State>,
) -> Result<Vec<C>, SimError> {
    let n = net.size;
    let mut a = vec![vec![C::default(); n]; n];
    let mut b = vec![C::default(); n];
    for (i, row) in a.iter_mut().enumerate().take(net.nodes.len()) {
        row[i] = row[i] + C::new(GMIN, 0.0);
    }
    for (ei, e) in net.els.iter().enumerate() {
        let p = net.ix(&e.nodes[0]);
        let q = net.ix(&e.nodes[1]);
        let conductance = |a: &mut [Vec<C>], g: C| {
            stamp(a, p, p, g);
            stamp(a, q, q, g);
            stamp(a, p, q, C::new(0.0, 0.0) - g);
            stamp(a, q, p, C::new(0.0, 0.0) - g);
        };
        match e.kind {
            Kind::R => {
                let r = Net::number(e)?;
                if r == 0.0 {
                    return Err(SimError::Circuit(format!(
                        "{} has zero resistance; use a wire",
                        e.name
                    )));
                }
                conductance(&mut a, C::new(1.0 / r, 0.0));
            }
            Kind::C => {
                let cap = Net::number(e)?;
                match mode {
                    Mode::Dc => {}
                    Mode::Ac(w) => conductance(&mut a, C::new(0.0, w * cap)),
                    Mode::Tran { h, be } => {
                        let g = if *be { cap / h } else { 2.0 * cap / h };
                        conductance(&mut a, C::new(g, 0.0));
                        let st =
                            state.ok_or_else(|| SimError::Failed("no previous state".into()))?;
                        let vprev = p.map_or(0.0, |i| st.v[i]) - q.map_or(0.0, |i| st.v[i]);
                        let ieq = g * vprev
                            + if *be {
                                0.0
                            } else {
                                st.cap_i.get(&ei).copied().unwrap_or(0.0)
                            };
                        if let Some(p) = p {
                            b[p] = b[p] + C::new(ieq, 0.0);
                        }
                        if let Some(q) = q {
                            b[q] = b[q] - C::new(ieq, 0.0);
                        }
                    }
                }
            }
            Kind::L | Kind::V => {
                let k = net.branch[&ei];
                // KCL: the branch current leaves p and enters q.
                stamp(&mut a, p, Some(k), C::new(1.0, 0.0));
                stamp(&mut a, q, Some(k), C::new(-1.0, 0.0));
                // Branch equation: v_p - v_q - Z i = rhs.
                stamp(&mut a, Some(k), p, C::new(1.0, 0.0));
                stamp(&mut a, Some(k), q, C::new(-1.0, 0.0));
                if e.kind == Kind::L {
                    let l = Net::number(e)?;
                    match mode {
                        Mode::Dc => {}
                        Mode::Ac(w) => a[k][k] = a[k][k] - C::new(0.0, w * l),
                        Mode::Tran { h, be } => {
                            let z = if *be { l / h } else { 2.0 * l / h };
                            a[k][k] = a[k][k] - C::new(z, 0.0);
                            let st = state
                                .ok_or_else(|| SimError::Failed("no previous state".into()))?;
                            let vprev = if *be {
                                0.0
                            } else {
                                st.ind_v.get(&ei).copied().unwrap_or(0.0)
                            };
                            let rhs = -z * st.ind_i.get(&ei).copied().unwrap_or(0.0) - vprev;
                            b[k] = C::new(rhs, 0.0);
                        }
                    }
                } else {
                    b[k] = src(ei, Net::source(e)?);
                }
            }
            Kind::I => {
                let v = src(ei, Net::source(e)?);
                if let Some(p) = p {
                    b[p] = b[p] - v;
                }
                if let Some(q) = q {
                    b[q] = b[q] + v;
                }
            }
            _ => {}
        }
    }
    solve(a, b)
}

fn dc_value(_: usize, s: &SourceSpec) -> C {
    C::new(s.dc, 0.0)
}

/// SPICE names are case-insensitive.
fn find_source(c: &Circuit, name: &str) -> Result<usize, SimError> {
    c.elements
        .iter()
        .position(|e| e.name.eq_ignore_ascii_case(name) && matches!(e.kind, Kind::V | Kind::I))
        .ok_or_else(|| {
            SimError::Circuit(format!(
                "“{name}” is not a voltage or current source of this circuit"
            ))
        })
}

fn voltage_series(net: &Net<'_>, c: &Circuit, cols: &[Vec<f64>]) -> Vec<Series> {
    // `cols[k][u]` = unknown u at point k.
    let mut out = Vec::new();
    for (i, n) in net.nodes.iter().enumerate() {
        out.push(Series {
            name: format!("v({n})"),
            unit: "V",
            values: cols.iter().map(|c| c[i]).collect(),
        });
    }
    for (ei, k) in &net.branch {
        out.push(Series {
            name: format!("i({})", c.elements[*ei].name),
            unit: "A",
            values: cols.iter().map(|c| c[*k]).collect(),
        });
    }
    out
}

pub struct BuiltinBackend;

impl Backend for BuiltinBackend {
    fn id(&self) -> &'static str {
        "builtin"
    }
    fn name(&self) -> &'static str {
        "Built-in linear engine"
    }
    fn available(&self) -> bool {
        true
    }

    fn run(&self, c: &Circuit, analysis: &Analysis) -> Result<SimResult, SimError> {
        c.validate()?;
        analysis.validate()?;
        let net = Net::new(c)?;
        let mut res = SimResult {
            analysis: analysis.name().into(),
            backend: self.name().into(),
            ..SimResult::default()
        };
        match analysis {
            Analysis::Op => {
                let x = assemble_solve(&net, &Mode::Dc, &dc_value, None)?;
                let col: Vec<f64> = x.iter().map(|c| c.re).collect();
                res.series = voltage_series(&net, c, &[col]);
            }
            Analysis::DcSweep {
                source,
                start,
                stop,
                step,
            } => {
                let si = find_source(c, source)?;
                let n = ((stop - start) / step).abs().round() as usize;
                let mut cols = Vec::with_capacity(n + 1);
                for k in 0..=n {
                    let v = start + step * k as f64;
                    let x = assemble_solve(
                        &net,
                        &Mode::Dc,
                        &|i, s| C::new(if i == si { v } else { s.dc }, 0.0),
                        None,
                    )?;
                    res.x.push(v);
                    cols.push(x.iter().map(|c| c.re).collect::<Vec<f64>>());
                }
                res.x_label = c.elements[si].name.to_lowercase();
                res.x_unit = if c.elements[si].kind == Kind::V {
                    "V"
                } else {
                    "A"
                };
                res.series = voltage_series(&net, c, &cols);
            }
            Analysis::Ac {
                points_per_decade,
                start_hz,
                stop_hz,
            } => {
                if !c
                    .elements
                    .iter()
                    .any(|e| matches!(&e.value, Value::Source(s) if s.ac_mag != 0.0))
                {
                    return Err(SimError::Circuit(
                        "no source has an AC magnitude (set `AC 1` on the input source)".into(),
                    ));
                }
                let decades = (stop_hz / start_hz).log10();
                let n = (decades * f64::from(*points_per_decade)).ceil() as usize;
                let ac = |_: usize, s: &SourceSpec| {
                    let ph = s.ac_phase_deg.to_radians();
                    C::new(s.ac_mag * ph.cos(), s.ac_mag * ph.sin())
                };
                let mut mags: Vec<Vec<f64>> = Vec::new();
                let mut phs: Vec<Vec<f64>> = Vec::new();
                for k in 0..=n {
                    let f = (start_hz * 10f64.powf(k as f64 / f64::from(*points_per_decade)))
                        .min(*stop_hz);
                    let x = assemble_solve(&net, &Mode::Ac(2.0 * PI * f), &ac, None)?;
                    res.x.push(f);
                    mags.push(
                        x.iter()
                            .map(|z| 20.0 * z.abs().max(1e-300).log10())
                            .collect(),
                    );
                    phs.push(x.iter().map(|z| z.arg().to_degrees()).collect());
                }
                res.x_label = "frequency".into();
                res.x_unit = "Hz";
                res.x_log = true;
                for (i, nname) in net.nodes.iter().enumerate() {
                    res.series.push(Series {
                        name: format!("mag(v({nname}))"),
                        unit: "dB",
                        values: mags.iter().map(|m| m[i]).collect(),
                    });
                    res.series.push(Series {
                        name: format!("phase(v({nname}))"),
                        unit: "deg",
                        values: phs.iter().map(|m| m[i]).collect(),
                    });
                }
            }
            Analysis::Tran { step, stop } => {
                let steps = (stop / step).round() as usize;
                let x0 = assemble_solve(&net, &Mode::Dc, &|_, s| C::new(s.at(0.0), 0.0), None)?;
                let mut cur: Vec<f64> = x0.iter().map(|z| z.re).collect();
                let mut st = State {
                    v: cur[..net.nodes.len()].to_vec(),
                    cap_i: BTreeMap::new(),
                    ind_i: BTreeMap::new(),
                    ind_v: BTreeMap::new(),
                };
                for (ei, k) in &net.branch {
                    if c.elements[*ei].kind == Kind::L {
                        st.ind_i.insert(*ei, cur[*k]);
                        st.ind_v.insert(*ei, 0.0);
                    }
                }
                let mut cols = vec![cur.clone()];
                res.x.push(0.0);
                for k in 1..=steps {
                    let t = *step * k as f64;
                    // Backward Euler across abrupt source edges (the trapezoidal rule would ring
                    // there), trapezoidal everywhere else.
                    let be = c.elements.iter().any(|e| match &e.value {
                        Value::Source(sp) => {
                            let scale = match &sp.wave {
                                Some(crate::circuit::Wave::Pulse { v1, v2, .. }) => (v2 - v1).abs(),
                                Some(crate::circuit::Wave::Sin { amp, .. }) => amp.abs(),
                                None => 0.0,
                            };
                            scale > 0.0 && (sp.at(t) - sp.at(t - step)).abs() > 0.25 * scale
                        }
                        _ => false,
                    });
                    let x = assemble_solve(
                        &net,
                        &Mode::Tran { h: *step, be },
                        &|_, s| C::new(s.at(t), 0.0),
                        Some(&st),
                    )?;
                    cur = x.iter().map(|z| z.re).collect();
                    for (ei, e) in c.elements.iter().enumerate() {
                        let pv = net.ix(&e.nodes[0]).map_or(0.0, |i| cur[i]);
                        let qv = net.ix(&e.nodes[1]).map_or(0.0, |i| cur[i]);
                        match e.kind {
                            Kind::C => {
                                let cap = Net::number(e)?;
                                let g = if be { cap / step } else { 2.0 * cap / step };
                                let vprev = net.ix(&e.nodes[0]).map_or(0.0, |i| st.v[i])
                                    - net.ix(&e.nodes[1]).map_or(0.0, |i| st.v[i]);
                                let ieq = g * vprev
                                    + if be {
                                        0.0
                                    } else {
                                        st.cap_i.get(&ei).copied().unwrap_or(0.0)
                                    };
                                st.cap_i.insert(ei, g * (pv - qv) - ieq);
                            }
                            Kind::L => {
                                st.ind_i.insert(ei, cur[net.branch[&ei]]);
                                st.ind_v.insert(ei, pv - qv);
                            }
                            _ => {}
                        }
                    }
                    st.v = cur[..net.nodes.len()].to_vec();
                    res.x.push(t);
                    cols.push(cur.clone());
                }
                res.x_label = "time".into();
                res.x_unit = "s";
                res.series = voltage_series(&net, c, &cols);
            }
        }
        Ok(res)
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;

    fn el(name: &str, kind: Kind, nodes: &[&str], value: Value) -> Element {
        Element {
            name: name.into(),
            kind,
            nodes: nodes.iter().map(|s| s.to_string()).collect(),
            value,
        }
    }
    fn num(v: f64) -> Value {
        Value::Number(v)
    }
    fn src(s: &str) -> Value {
        Value::Source(SourceSpec::parse(s).unwrap())
    }
    fn circuit(els: Vec<Element>) -> Circuit {
        Circuit {
            title: "t".into(),
            elements: els,
            models: vec![],
        }
    }
    fn rc(vsrc: &str) -> Circuit {
        circuit(vec![
            el("V1", Kind::V, &["in", "0"], src(vsrc)),
            el("R1", Kind::R, &["in", "out"], num(1000.0)),
            el("C1", Kind::C, &["out", "0"], num(1e-6)),
        ])
    }

    #[test]
    fn operating_point_of_a_divider() {
        let c = circuit(vec![
            el("V1", Kind::V, &["in", "0"], src("10")),
            el("R1", Kind::R, &["in", "mid"], num(1000.0)),
            el("R2", Kind::R, &["mid", "0"], num(3000.0)),
        ]);
        let r = BuiltinBackend.run(&c, &Analysis::Op).unwrap();
        assert!((r.scalar("v(mid)").unwrap() - 7.5).abs() < 1e-6);
        // SPICE sign convention: the current into the + terminal of the source is negative here.
        assert!((r.scalar("i(V1)").unwrap() + 2.5e-3).abs() < 1e-9);
    }

    #[test]
    fn dc_sweep_is_linear_in_the_swept_source() {
        let c = circuit(vec![
            el("V1", Kind::V, &["in", "0"], src("0")),
            el("R1", Kind::R, &["in", "mid"], num(1000.0)),
            el("R2", Kind::R, &["mid", "0"], num(1000.0)),
        ]);
        let r = BuiltinBackend
            .run(
                &c,
                &Analysis::DcSweep {
                    source: "v1".into(),
                    start: 0.0,
                    stop: 10.0,
                    step: 2.0,
                },
            )
            .unwrap();
        assert_eq!(r.x, [0.0, 2.0, 4.0, 6.0, 8.0, 10.0]);
        for (x, v) in r.x.iter().zip(&r.series("v(mid)").unwrap().values) {
            assert!((v - x / 2.0).abs() < 1e-6);
        }
        assert!(BuiltinBackend
            .run(
                &c,
                &Analysis::DcSweep {
                    source: "R1".into(),
                    start: 0.0,
                    stop: 1.0,
                    step: 1.0
                }
            )
            .is_err());
    }

    #[test]
    fn ac_rc_lowpass_matches_the_analytic_response() {
        let r = BuiltinBackend
            .run(
                &rc("AC 1"),
                &Analysis::Ac {
                    points_per_decade: 20,
                    start_hz: 1.0,
                    stop_hz: 1e5,
                },
            )
            .unwrap();
        let fc = 1.0 / (2.0 * PI * 1000.0 * 1e-6);
        let mag = r.series("mag(v(out))").unwrap();
        let ph = r.series("phase(v(out))").unwrap();
        for (i, f) in r.x.iter().enumerate() {
            let w = f / fc;
            let want_db = -10.0 * (1.0 + w * w).log10();
            let want_ph = -w.atan().to_degrees();
            assert!(
                (mag.values[i] - want_db).abs() < 1e-6,
                "f={f}: {} vs {want_db}",
                mag.values[i]
            );
            assert!((ph.values[i] - want_ph).abs() < 1e-6, "f={f}");
        }
        assert!(
            r.x_log && (r.x[0] - 1.0).abs() < 1e-12 && (r.x.last().unwrap() - 1e5).abs() < 1e-6
        );
        assert!(
            BuiltinBackend
                .run(
                    &rc("5"),
                    &Analysis::Ac {
                        points_per_decade: 5,
                        start_hz: 1.0,
                        stop_hz: 10.0
                    }
                )
                .is_err(),
            "no AC source"
        );
    }

    #[test]
    fn transient_rc_step_follows_the_exponential() {
        let r = BuiltinBackend
            .run(
                &rc("PULSE(0 5 0 1n 1n 1 2)"),
                &Analysis::Tran {
                    step: 1e-5,
                    stop: 5e-3,
                },
            )
            .unwrap();
        let out = r.series("v(out)").unwrap();
        let tau = 1e-3;
        for (t, v) in r.x.iter().zip(&out.values) {
            if *t < 1e-4 {
                continue;
            }
            let want = 5.0 * (1.0 - (-t / tau).exp());
            assert!((v - want).abs() < 0.01, "t={t}: {v} vs {want}");
        }
        assert!((out.values.last().unwrap() - 5.0 * (1.0 - (-5.0f64).exp())).abs() < 0.01);
    }

    #[test]
    fn transient_rl_current_and_sine_through_resistor() {
        let rl = circuit(vec![
            el("V1", Kind::V, &["in", "0"], src("PULSE(0 1 0 1n 1n 1 2)")),
            el("R1", Kind::R, &["in", "x"], num(10.0)),
            el("L1", Kind::L, &["x", "0"], num(10e-3)),
        ]);
        let r = BuiltinBackend
            .run(
                &rl,
                &Analysis::Tran {
                    step: 1e-5,
                    stop: 5e-3,
                },
            )
            .unwrap();
        let i = r.series("i(L1)").unwrap();
        let tau = 10e-3 / 10.0;
        let last_t = *r.x.last().unwrap();
        let want = 0.1 * (1.0 - (-last_t / tau).exp());
        assert!((i.values.last().unwrap() - want).abs() < 1e-4);
        let sine = circuit(vec![
            el("V1", Kind::V, &["in", "0"], src("SIN(0 2 1k)")),
            el("R1", Kind::R, &["in", "0"], num(100.0)),
        ]);
        let r = BuiltinBackend
            .run(
                &sine,
                &Analysis::Tran {
                    step: 1e-5,
                    stop: 1e-3,
                },
            )
            .unwrap();
        let v = r.series("v(in)").unwrap();
        let peak = v.values.iter().cloned().fold(f64::MIN, f64::max);
        assert!((peak - 2.0).abs() < 0.01);
    }

    #[test]
    fn unsupported_and_broken_circuits_are_reported() {
        let diode = circuit(vec![
            el("V1", Kind::V, &["in", "0"], src("5")),
            el("D1", Kind::D, &["in", "0"], Value::Model("D1N4148".into())),
        ]);
        assert!(matches!(
            BuiltinBackend.run(&diode, &Analysis::Op),
            Err(SimError::Unsupported { .. })
        ));
        let floating = circuit(vec![
            el("V1", Kind::V, &["a", "0"], src("5")),
            el("R1", Kind::R, &["b", "c"], num(1000.0)),
            el("R2", Kind::R, &["a", "0"], num(1000.0)),
        ]);
        let r = BuiltinBackend.run(&floating, &Analysis::Op);
        // gmin keeps floating pieces solvable (as in SPICE); they sit at 0 V.
        assert!((r.unwrap().scalar("v(b)").unwrap()).abs() < 1e-6);
        let loop_v = circuit(vec![
            el("V1", Kind::V, &["a", "0"], src("5")),
            el("V2", Kind::V, &["a", "0"], src("3")),
        ]);
        assert!(matches!(
            BuiltinBackend.run(&loop_v, &Analysis::Op),
            Err(SimError::Failed(_))
        ));
        let zero = circuit(vec![
            el("V1", Kind::V, &["a", "0"], src("5")),
            el("R1", Kind::R, &["a", "0"], num(0.0)),
        ]);
        assert!(BuiltinBackend.run(&zero, &Analysis::Op).is_err());
        assert!(BuiltinBackend
            .run(
                &rc("5"),
                &Analysis::Tran {
                    step: 0.0,
                    stop: 1.0
                }
            )
            .is_err());
    }
}
