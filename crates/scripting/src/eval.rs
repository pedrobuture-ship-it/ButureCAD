//! Semantic analysis and elaboration: AST → validated [`Design`] (the EDA IR).
//!
//! One pass evaluates variables, functions, loops, conditions and module instances (so the IR is
//! always flat), checks dimensions, resolves names, and records every problem as a diagnostic
//! with a source position. A second step validates references (pins, placements, layers).

use crate::ast::*;
use crate::diag::{Diagnostic, Span};
use crate::value::{format_qty, to_nm, unit, Dim, Qty, Val, UNIT_LIST};
use buturecad_eda_core::ir::{
    BoardIr, ComponentIr, Design, NetClassIr, NetIr, PinRefIr, PlacementIr, Pt, Side, TrackIr,
    ViaIr, ZoneIr,
};
use buturecad_schematic::builtin;
use std::collections::{BTreeMap, BTreeSet};

const MAX_STEPS: u64 = 2_000_000;
const MAX_LOOP: i64 = 100_000;
const MAX_MODULE_DEPTH: usize = 16;
const MAX_FN_DEPTH: usize = 64;
const GLOBAL_NETS: [&str; 4] = ["GND", "VCC", "+3V3", "+5V"];

type R<T> = Result<T, ()>;

#[derive(Default)]
struct Frame {
    prefix: String,
    ports: BTreeMap<String, String>,
    module: Option<String>,
}

struct CompEntry {
    ir: ComponentIr,
    span: Span,
    /// Pins of the builtin symbol, when known `(number, name)`.
    symbol_pins: Option<Vec<(String, String)>>,
    connected: bool,
    has_footprint: bool,
    placed: bool,
}

struct PendingPin {
    reference: String,
    pin: String,
    span: Span,
}

#[derive(Default)]
struct NetSets {
    parent: Vec<usize>,
    name: Vec<Option<(String, Span)>>,
    pins: Vec<Vec<(String, String)>>,
    pin_ids: BTreeMap<(String, String), usize>,
    name_ids: BTreeMap<String, usize>,
}

impl NetSets {
    fn new_set(&mut self) -> usize {
        self.parent.push(self.parent.len());
        self.name.push(None);
        self.pins.push(Vec::new());
        self.parent.len() - 1
    }
    fn find(&mut self, mut x: usize) -> usize {
        while self.parent[x] != x {
            self.parent[x] = self.parent[self.parent[x]];
            x = self.parent[x];
        }
        x
    }
    fn pin(&mut self, r: &str, p: &str) -> usize {
        let k = (r.to_owned(), p.to_owned());
        if let Some(i) = self.pin_ids.get(&k) {
            return *i;
        }
        let i = self.new_set();
        self.pins[i].push(k.clone());
        self.pin_ids.insert(k, i);
        i
    }
    fn named(&mut self, n: &str, span: Span) -> usize {
        if let Some(i) = self.name_ids.get(n) {
            return *i;
        }
        let i = self.new_set();
        self.name[i] = Some((n.to_owned(), span));
        self.name_ids.insert(n.to_owned(), i);
        i
    }
    /// Joins two sets; returns the conflicting names if both are named differently.
    fn union(&mut self, a: usize, b: usize) -> Result<(), (String, String)> {
        let (ra, rb) = (self.find(a), self.find(b));
        if ra == rb {
            return Ok(());
        }
        if let (Some((x, _)), Some((y, _))) = (&self.name[ra], &self.name[rb]) {
            if x != y {
                return Err((x.clone(), y.clone()));
            }
        }
        let moved = std::mem::take(&mut self.pins[rb]);
        self.pins[ra].extend(moved);
        if self.name[ra].is_none() {
            self.name[ra] = self.name[rb].take();
        }
        self.parent[rb] = ra;
        Ok(())
    }
}

pub struct Elaborator {
    diags: Vec<Diagnostic>,
    steps: u64,
    aborted: bool,
    scopes: Vec<BTreeMap<String, Val>>,
    fns: BTreeMap<String, (Vec<String>, Expr)>,
    modules: BTreeMap<String, (Vec<Param>, Vec<Item>)>,
    overrides: BTreeMap<String, Val>,
    project: Option<String>,
    board: Option<(BoardIr, Span)>,
    comps: BTreeMap<String, CompEntry>,
    order: Vec<String>,
    nets: NetSets,
    pins: Vec<PendingPin>,
    placements: Vec<(PlacementIr, Span)>,
    tracks: Vec<(TrackIr, Span)>,
    vias: Vec<ViaIr>,
    zones: Vec<(ZoneIr, Span)>,
    rules: buturecad_eda_core::ir::RulesIr,
    classes: Vec<NetClassIr>,
    module_depth: usize,
    fn_depth: usize,
}

fn builtin_pins(symbol: &str) -> Option<Vec<(String, String)>> {
    builtin::symbols()
        .into_iter()
        .find(|s| s.name == symbol)
        .map(|s| s.pins.into_iter().map(|p| (p.number, p.name)).collect())
}

impl Elaborator {
    pub fn new(overrides: BTreeMap<String, Val>) -> Self {
        Self {
            diags: Vec::new(),
            steps: 0,
            aborted: false,
            scopes: vec![BTreeMap::new()],
            fns: BTreeMap::new(),
            modules: BTreeMap::new(),
            overrides,
            project: None,
            board: None,
            comps: BTreeMap::new(),
            order: Vec::new(),
            nets: NetSets::default(),
            pins: Vec::new(),
            placements: Vec::new(),
            tracks: Vec::new(),
            vias: Vec::new(),
            zones: Vec::new(),
            rules: Default::default(),
            classes: Vec::new(),
            module_depth: 0,
            fn_depth: 0,
        }
    }

    fn error<T>(&mut self, code: &'static str, span: Span, msg: impl Into<String>) -> R<T> {
        self.diags.push(Diagnostic::error(code, span, msg));
        Err(())
    }

    fn step(&mut self, span: Span) -> R<()> {
        self.steps += 1;
        if self.steps > MAX_STEPS {
            if !self.aborted {
                self.aborted = true;
                self.diags.push(Diagnostic::error(
                    "E-limit",
                    span,
                    "the script needs more than 2 000 000 evaluation steps (runaway loop or recursion?)",
                ));
            }
            return Err(());
        }
        Ok(())
    }

    // -- expressions -----------------------------------------------------------------------------

    fn lookup(&self, name: &str) -> Option<&Val> {
        self.scopes.iter().rev().find_map(|s| s.get(name))
    }

    #[allow(clippy::result_unit_err)]
    pub fn eval(&mut self, e: &Expr) -> R<Val> {
        self.step(e.span)?;
        match &e.kind {
            ExprKind::Num(v, None) => Ok(Val::Q(Qty::plain(*v))),
            ExprKind::Num(v, Some(u)) => match unit(u) {
                Some((f, d)) => Ok(Val::Q(Qty { v: v * f, dim: d })),
                None => self.error(
                    "E-unit",
                    e.span,
                    format!("unknown unit “{u}” (known: {UNIT_LIST})"),
                ),
            },
            ExprKind::Str(s) => Ok(Val::Str(s.clone())),
            ExprKind::Bool(b) => Ok(Val::Bool(*b)),
            ExprKind::Ident(n) => match self.lookup(n) {
                Some(v) => Ok(v.clone()),
                None => self.error("E-undefined", e.span, format!("unknown name “{n}”")),
            },
            ExprKind::Tuple(v) => Ok(Val::Tuple(
                v.iter().map(|x| self.eval(x)).collect::<R<_>>()?,
            )),
            ExprKind::List(v) => Ok(Val::List(v.iter().map(|x| self.eval(x)).collect::<R<_>>()?)),
            ExprKind::Unary(op, x) => {
                let v = self.eval(x)?;
                match (op, v) {
                    (Op::Neg, Val::Q(q)) => Ok(Val::Q(Qty {
                        v: -q.v,
                        dim: q.dim,
                    })),
                    (Op::Not, Val::Bool(b)) => Ok(Val::Bool(!b)),
                    (Op::Neg, o) => {
                        self.error("E-type", e.span, format!("cannot negate {}", o.type_name()))
                    }
                    (_, o) => self.error(
                        "E-type",
                        e.span,
                        format!("“!” needs a boolean, found {}", o.type_name()),
                    ),
                }
            }
            ExprKind::Binary(op, a, b) => {
                // Short-circuit logic first.
                if matches!(op, Op::And | Op::Or) {
                    let l = self.eval(a)?;
                    let Val::Bool(lb) = l else {
                        return self.error(
                            "E-type",
                            a.span,
                            format!("expected a boolean, found {}", l.type_name()),
                        );
                    };
                    if (*op == Op::And && !lb) || (*op == Op::Or && lb) {
                        return Ok(Val::Bool(lb));
                    }
                    let r = self.eval(b)?;
                    return match r {
                        Val::Bool(rb) => Ok(Val::Bool(rb)),
                        o => self.error(
                            "E-type",
                            b.span,
                            format!("expected a boolean, found {}", o.type_name()),
                        ),
                    };
                }
                let (l, r) = (self.eval(a)?, self.eval(b)?);
                self.binary(*op, l, r, e.span)
            }
            ExprKind::Call(name, args) => self.call(name, args, e.span),
        }
    }

    fn binary(&mut self, op: Op, l: Val, r: Val, span: Span) -> R<Val> {
        let sym = |o: Op| match o {
            Op::Add => "+",
            Op::Sub => "-",
            Op::Mul => "*",
            Op::Div => "/",
            Op::Rem => "%",
            Op::Eq => "==",
            Op::Ne => "!=",
            Op::Lt => "<",
            Op::Le => "<=",
            Op::Gt => ">",
            _ => ">=",
        };
        match (l, r) {
            (Val::Q(a), Val::Q(b)) => match op {
                Op::Add
                | Op::Sub
                | Op::Rem
                | Op::Eq
                | Op::Ne
                | Op::Lt
                | Op::Le
                | Op::Gt
                | Op::Ge
                    if a.dim != b.dim =>
                {
                    self.error(
                        "E-dimension",
                        span,
                        format!("incompatible dimensions: {} {} {}", a.dim, sym(op), b.dim),
                    )
                }
                Op::Add => Ok(Val::Q(Qty {
                    v: a.v + b.v,
                    dim: a.dim,
                })),
                Op::Sub => Ok(Val::Q(Qty {
                    v: a.v - b.v,
                    dim: a.dim,
                })),
                Op::Rem => {
                    if b.v == 0.0 {
                        return self.error("E-math", span, "remainder by zero");
                    }
                    Ok(Val::Q(Qty {
                        v: a.v % b.v,
                        dim: a.dim,
                    }))
                }
                Op::Mul => Ok(Val::Q(Qty {
                    v: a.v * b.v,
                    dim: a.dim.times(b.dim),
                })),
                Op::Div => {
                    if b.v == 0.0 {
                        return self.error("E-math", span, "division by zero");
                    }
                    Ok(Val::Q(Qty {
                        v: a.v / b.v,
                        dim: a.dim.times(b.dim.inv()),
                    }))
                }
                Op::Eq => Ok(Val::Bool(
                    (a.v - b.v).abs() <= f64::EPSILON * a.v.abs().max(1.0),
                )),
                Op::Ne => Ok(Val::Bool(
                    (a.v - b.v).abs() > f64::EPSILON * a.v.abs().max(1.0),
                )),
                Op::Lt => Ok(Val::Bool(a.v < b.v)),
                Op::Le => Ok(Val::Bool(a.v <= b.v)),
                Op::Gt => Ok(Val::Bool(a.v > b.v)),
                Op::Ge => Ok(Val::Bool(a.v >= b.v)),
                _ => self.error("E-type", span, "bad operator"),
            },
            (Val::Str(a), Val::Str(b)) => match op {
                Op::Add => Ok(Val::Str(a + &b)),
                Op::Eq => Ok(Val::Bool(a == b)),
                Op::Ne => Ok(Val::Bool(a != b)),
                _ => self.error(
                    "E-type",
                    span,
                    format!("“{}” does not work on strings", sym(op)),
                ),
            },
            (Val::Str(a), Val::Q(q)) if op == Op::Add => Ok(Val::Str(a + &format_qty(q))),
            (Val::Q(q), Val::Str(b)) if op == Op::Add => Ok(Val::Str(format_qty(q) + &b)),
            (Val::Bool(a), Val::Bool(b)) if matches!(op, Op::Eq | Op::Ne) => {
                Ok(Val::Bool((a == b) == (op == Op::Eq)))
            }
            (a, b) => self.error(
                "E-type",
                span,
                format!(
                    "cannot apply “{}” to {} and {}",
                    sym(op),
                    a.type_name(),
                    b.type_name()
                ),
            ),
        }
    }

    fn call(&mut self, name: &str, args: &[Expr], span: Span) -> R<Val> {
        let vals: Vec<Val> = args.iter().map(|a| self.eval(a)).collect::<R<_>>()?;
        if let Some((params, body)) = self.fns.get(name).cloned() {
            if params.len() != vals.len() {
                return self.error(
                    "E-arity",
                    span,
                    format!(
                        "{name} takes {} argument(s), got {}",
                        params.len(),
                        vals.len()
                    ),
                );
            }
            if self.fn_depth >= MAX_FN_DEPTH {
                return self.error(
                    "E-limit",
                    span,
                    "function calls nest too deeply (recursion without end?)",
                );
            }
            self.fn_depth += 1;
            // A function sees the global definitions and its own parameters only.
            let globals = self.scopes[0].clone();
            let saved = std::mem::replace(
                &mut self.scopes,
                vec![globals, params.into_iter().zip(vals).collect()],
            );
            let r = self.eval(&body);
            self.scopes = saved;
            self.fn_depth -= 1;
            return r;
        }
        let want = |n: usize, this: &mut Self| -> R<()> {
            if vals.len() == n {
                Ok(())
            } else {
                this.error(
                    "E-arity",
                    span,
                    format!("{name} takes {n} argument(s), got {}", vals.len()),
                )
            }
        };
        match name {
            "rect" => {
                want(4, self)?;
                let q: Vec<Qty> = vals
                    .iter()
                    .map(|v| match v {
                        Val::Q(q) if q.dim == Dim::LENGTH => Ok(*q),
                        o => self.error(
                            "E-dimension",
                            span,
                            format!("rect() needs lengths, found {}", o.type_name()),
                        ),
                    })
                    .collect::<R<_>>()?;
                let p = |x: Qty, y: Qty| Val::Tuple(vec![Val::Q(x), Val::Q(y)]);
                Ok(Val::List(vec![
                    p(q[0], q[1]),
                    p(q[2], q[1]),
                    p(q[2], q[3]),
                    p(q[0], q[3]),
                ]))
            }
            "abs" | "sqrt" | "floor" | "round" => {
                want(1, self)?;
                let Val::Q(q) = &vals[0] else {
                    return self.error("E-type", span, format!("{name}() needs a number"));
                };
                Ok(Val::Q(match name {
                    "abs" => Qty {
                        v: q.v.abs(),
                        dim: q.dim,
                    },
                    "floor" => Qty {
                        v: q.v.floor(),
                        dim: q.dim,
                    },
                    "round" => Qty {
                        v: q.v.round(),
                        dim: q.dim,
                    },
                    _ => {
                        if q.v < 0.0 || q.dim.0.iter().any(|e| e % 2 != 0) {
                            return self.error(
                                "E-dimension",
                                span,
                                format!("sqrt() of {} is not defined", q.dim),
                            );
                        }
                        Qty {
                            v: q.v.sqrt(),
                            dim: Dim(q.dim.0.map(|e| e / 2)),
                        }
                    }
                }))
            }
            "min" | "max" => {
                want(2, self)?;
                match (&vals[0], &vals[1]) {
                    (Val::Q(a), Val::Q(b)) if a.dim == b.dim => {
                        Ok(Val::Q(if (name == "min") == (a.v <= b.v) {
                            *a
                        } else {
                            *b
                        }))
                    }
                    (Val::Q(a), Val::Q(b)) => self.error(
                        "E-dimension",
                        span,
                        format!("incompatible dimensions: {} and {}", a.dim, b.dim),
                    ),
                    _ => self.error("E-type", span, format!("{name}() needs two numbers")),
                }
            }
            "str" => {
                want(1, self)?;
                match &vals[0] {
                    Val::Q(q) => Ok(Val::Str(format_qty(*q))),
                    Val::Str(s) => Ok(Val::Str(s.clone())),
                    o => self.error(
                        "E-type",
                        span,
                        format!("str() cannot convert {}", o.type_name()),
                    ),
                }
            }
            _ => self.error("E-undefined", span, format!("unknown function “{name}”")),
        }
    }

    // -- value extraction ------------------------------------------------------------------------

    fn length(&mut self, e: &Expr, what: &str) -> R<i64> {
        match self.eval(e)? {
            Val::Q(q) => match to_nm(q) {
                Ok(n) => Ok(n),
                Err(m) => self.error(
                    if q.dim == Dim::LENGTH {
                        "E-precision"
                    } else {
                        "E-dimension"
                    },
                    e.span,
                    format!("{what}: {m}"),
                ),
            },
            o => self.error(
                "E-type",
                e.span,
                format!("{what}: expected a length, found {}", o.type_name()),
            ),
        }
    }

    fn point_from(&mut self, v: &Val, span: Span, what: &str) -> R<Pt> {
        match v {
            Val::Tuple(t) if t.len() == 2 => {
                let mut c = [0i64; 2];
                for (i, x) in t.iter().enumerate() {
                    c[i] = match x {
                        Val::Q(q) => match to_nm(*q) {
                            Ok(n) => n,
                            Err(m) => {
                                return self.error("E-dimension", span, format!("{what}: {m}"))
                            }
                        },
                        o => {
                            return self.error(
                                "E-type",
                                span,
                                format!(
                                    "{what}: coordinates must be lengths, found {}",
                                    o.type_name()
                                ),
                            )
                        }
                    };
                }
                Ok(Pt { x: c[0], y: c[1] })
            }
            o => self.error(
                "E-type",
                span,
                format!("{what}: expected a point (x, y), found {}", o.type_name()),
            ),
        }
    }

    fn point(&mut self, e: &Expr, what: &str) -> R<Pt> {
        let v = self.eval(e)?;
        self.point_from(&v, e.span, what)
    }

    fn polygon(&mut self, e: &Expr, what: &str) -> R<Vec<Pt>> {
        match self.eval(e)? {
            Val::List(l) => l.iter().map(|v| self.point_from(v, e.span, what)).collect(),
            o => self.error(
                "E-type",
                e.span,
                format!("{what}: expected a list of points, found {}", o.type_name()),
            ),
        }
    }

    fn string(&mut self, e: &Expr, what: &str) -> R<String> {
        match self.eval(e)? {
            Val::Str(s) => Ok(s),
            Val::Q(q) if q.dim.is_none() || q.dim != Dim::LENGTH => Ok(format_qty(q)),
            o => self.error(
                "E-type",
                e.span,
                format!("{what}: expected text, found {}", o.type_name()),
            ),
        }
    }

    fn integer(&mut self, e: &Expr, what: &str) -> R<i64> {
        match self.eval(e)? {
            Val::Q(q) if q.dim.is_none() && q.v.fract() == 0.0 && q.v.abs() < 1e12 => {
                Ok(q.v as i64)
            }
            o => self.error(
                "E-type",
                e.span,
                format!("{what}: expected a whole number, found {}", o.type_name()),
            ),
        }
    }

    // -- names -----------------------------------------------------------------------------------

    fn local_name(&mut self, n: &NameRef) -> R<String> {
        match &n.index {
            None => Ok(n.base.clone()),
            Some(e) => {
                let i = self.integer(e, "index")?;
                Ok(format!("{}{i}", n.base))
            }
        }
    }

    fn comp_ref(&mut self, n: &NameRef, f: &Frame) -> R<String> {
        Ok(format!("{}{}", f.prefix, self.local_name(n)?))
    }

    fn net_name(&mut self, n: &NameRef, f: &Frame) -> R<String> {
        let local = self.local_name(n)?;
        if let Some(b) = f.ports.get(&local) {
            return Ok(b.clone());
        }
        if f.prefix.is_empty() {
            return Ok(local);
        }
        if GLOBAL_NETS.contains(&local.as_str()) {
            self.diags.push(Diagnostic::warning(
                "W-global",
                n.span,
                format!(
                    "“{local}” inside module {} is a local net (“{}{local}”); declare `port {local}` to connect it to the outside",
                    f.module.clone().unwrap_or_default(),
                    f.prefix
                ),
            ));
        }
        Ok(format!("{}{local}", f.prefix))
    }

    // -- statements ------------------------------------------------------------------------------

    #[allow(clippy::collapsible_match)]
    pub fn run(&mut self, program: &Program) {
        // Hoist declarations so order does not matter for functions and modules.
        for it in &program.items {
            match &it.kind {
                ItemKind::Fn { name, params, body } => {
                    if self
                        .fns
                        .insert(name.clone(), (params.clone(), body.clone()))
                        .is_some()
                    {
                        self.diags.push(Diagnostic::error(
                            "E-duplicate",
                            it.span,
                            format!("function “{name}” is defined twice"),
                        ));
                    }
                }
                ItemKind::Module { name, params, body } => {
                    if self
                        .modules
                        .insert(name.clone(), (params.clone(), body.clone()))
                        .is_some()
                    {
                        self.diags.push(Diagnostic::error(
                            "E-duplicate",
                            it.span,
                            format!("module “{name}” is defined twice"),
                        ));
                    }
                }
                _ => {}
            }
        }
        let f = Frame::default();
        self.items(&program.items, &f, true);
        self.finish();
    }

    fn items(&mut self, items: &[Item], f: &Frame, top: bool) {
        for it in items {
            if self.aborted {
                return;
            }
            let _ = self.item(it, f, top);
        }
    }

    fn props<'a>(
        &mut self,
        props: &'a [Prop],
        known: &[&str],
        what: &str,
    ) -> BTreeMap<String, &'a Prop> {
        let mut m = BTreeMap::new();
        for p in props {
            if !known.is_empty() && !known.contains(&p.key.as_str()) {
                self.diags.push(Diagnostic::error(
                    "E-property",
                    p.key_span,
                    format!(
                        "{what} has no property “{}” (known: {})",
                        p.key,
                        known.join(", ")
                    ),
                ));
                continue;
            }
            if m.insert(p.key.clone(), p).is_some() {
                self.diags.push(Diagnostic::error(
                    "E-duplicate",
                    p.key_span,
                    format!("property “{}” is set twice", p.key),
                ));
            }
        }
        m
    }

    fn item(&mut self, it: &Item, f: &Frame, top: bool) -> R<()> {
        self.step(it.span)?;
        match &it.kind {
            ItemKind::Fn { .. } | ItemKind::Module { .. } => {
                if !top {
                    return self.error(
                        "E-scope",
                        it.span,
                        "functions and modules can only be defined at the top level",
                    );
                }
                Ok(())
            }
            ItemKind::Project(e) => {
                let n = self.string(e, "project name")?;
                self.project = Some(n);
                Ok(())
            }
            ItemKind::Let { name, value } => {
                let v = self.eval(value)?;
                self.scopes.last_mut().map(|s| s.insert(name.clone(), v));
                Ok(())
            }
            ItemKind::ParamDecl { name, value } => {
                if !top {
                    return self.error(
                        "E-scope",
                        it.span,
                        "`param` can only be declared at the top level",
                    );
                }
                let v = match self.overrides.remove(name) {
                    Some(v) => v,
                    None => self.eval(value)?,
                };
                self.scopes[0].insert(name.clone(), v);
                Ok(())
            }
            ItemKind::Board(props) => self.board_item(props, it.span),
            ItemKind::Component { name, props } => self.component(name, props, f, it.span),
            ItemKind::Net { name, pins } => {
                let net = self.net_name(name, f)?;
                let set = self.nets.named(&net, name.span);
                for p in pins {
                    let _ = self.endpoint(p, f, set);
                }
                Ok(())
            }
            ItemKind::Connect(groups) => {
                let anchor = self.nets.new_set();
                for g in groups {
                    for p in g {
                        let _ = self.endpoint(p, f, anchor);
                    }
                }
                Ok(())
            }
            ItemKind::Place {
                target,
                at,
                rotate,
                side,
            } => {
                let reference = self.comp_ref(target, f)?;
                let at = self.point(at, "place")?;
                let rotation_deg = match rotate {
                    Some(e) => self.integer(e, "rotate (degrees)")? as i32,
                    None => 0,
                };
                let side = match side {
                    None => Side::Top,
                    Some((s, _)) if s == "top" => Side::Top,
                    Some((s, _)) if s == "bottom" => Side::Bottom,
                    Some((s, sp)) => {
                        return self.error(
                            "E-property",
                            *sp,
                            format!("side must be top or bottom, not “{s}”"),
                        )
                    }
                };
                self.placements.push((
                    PlacementIr {
                        reference,
                        at,
                        rotation_deg,
                        side,
                    },
                    it.span,
                ));
                Ok(())
            }
            ItemKind::Track {
                net,
                layer,
                width,
                path,
            } => {
                let net = self.net_name(net, f)?;
                let width = self.length(width, "track width")?;
                let points: Vec<Pt> = path
                    .iter()
                    .map(|p| self.point(p, "track path"))
                    .collect::<R<_>>()?;
                if width <= 0 {
                    return self.error("E-value", it.span, "track width must be positive");
                }
                self.tracks.push((
                    TrackIr {
                        net,
                        layer: layer.clone(),
                        points,
                        width,
                    },
                    it.span,
                ));
                Ok(())
            }
            ItemKind::Via {
                net,
                at,
                diameter,
                drill,
            } => {
                let net = self.net_name(net, f)?;
                let at = self.point(at, "via")?;
                let diameter = match diameter {
                    Some(e) => self.length(e, "via diameter")?,
                    None => 800_000,
                };
                let drill = match drill {
                    Some(e) => self.length(e, "via drill")?,
                    None => 400_000,
                };
                if drill <= 0 || diameter <= drill {
                    return self.error("E-value", it.span, "via needs 0 < drill < diameter");
                }
                self.vias.push(ViaIr {
                    net,
                    at,
                    diameter,
                    drill,
                });
                Ok(())
            }
            ItemKind::Zone { net, layer, props } => {
                let net = self.net_name(net, f)?;
                let m = self.props(
                    props,
                    &["outline", "clearance", "min_width", "priority"],
                    "zone",
                );
                let outline = match m.get("outline") {
                    Some(p) => self.polygon(&p.value, "zone outline")?,
                    None => return self.error("E-property", it.span, "zone needs an `outline`"),
                };
                let clearance = m
                    .get("clearance")
                    .map(|p| self.length(&p.value, "zone clearance"))
                    .transpose()?;
                let min_width = m
                    .get("min_width")
                    .map(|p| self.length(&p.value, "zone min_width"))
                    .transpose()?;
                let priority = match m.get("priority") {
                    Some(p) => self.integer(&p.value, "zone priority")?.clamp(0, 255) as u8,
                    None => 0,
                };
                self.zones.push((
                    ZoneIr {
                        net,
                        layer: layer.clone(),
                        outline,
                        clearance,
                        min_width,
                        priority,
                    },
                    it.span,
                ));
                Ok(())
            }
            ItemKind::Rule(props) => {
                let m = self.props(
                    props,
                    &[
                        "clearance",
                        "min_track_width",
                        "min_via_diameter",
                        "min_via_drill",
                        "min_annular_ring",
                        "copper_to_edge",
                    ],
                    "rule",
                );
                for (k, p) in m {
                    let v = self.length(&p.value, &k)?;
                    let r = &mut self.rules;
                    match k.as_str() {
                        "clearance" => r.clearance = Some(v),
                        "min_track_width" => r.min_track_width = Some(v),
                        "min_via_diameter" => r.min_via_diameter = Some(v),
                        "min_via_drill" => r.min_via_drill = Some(v),
                        "min_annular_ring" => r.min_annular_ring = Some(v),
                        _ => r.copper_to_edge = Some(v),
                    }
                }
                Ok(())
            }
            ItemKind::Class { name, props } => {
                let m = self.props(
                    props,
                    &["width", "clearance", "via_diameter", "via_drill", "nets"],
                    "class",
                );
                let mut c = NetClassIr {
                    name: name.clone(),
                    track_width: None,
                    clearance: None,
                    via_diameter: None,
                    via_drill: None,
                    nets: Vec::new(),
                };
                for (k, p) in m {
                    match k.as_str() {
                        "nets" => {
                            let ExprKind::List(items) = &p.value.kind else {
                                return self.error(
                                    "E-type",
                                    p.value.span,
                                    "nets must be a list such as [VCC, GND]",
                                );
                            };
                            for x in items {
                                let n = match &x.kind {
                                    ExprKind::Ident(n) if self.lookup(n).is_none() => n.clone(),
                                    _ => self.string(x, "net name")?,
                                };
                                c.nets.push(n);
                            }
                        }
                        "width" => c.track_width = Some(self.length(&p.value, "width")?),
                        "clearance" => c.clearance = Some(self.length(&p.value, "clearance")?),
                        "via_diameter" => {
                            c.via_diameter = Some(self.length(&p.value, "via_diameter")?)
                        }
                        _ => c.via_drill = Some(self.length(&p.value, "via_drill")?),
                    }
                }
                self.classes.push(c);
                Ok(())
            }
            ItemKind::Use { name, module, args } => self.use_module(name, module, args, f, it.span),
            ItemKind::For {
                var,
                from,
                to,
                body,
            } => {
                let (a, b) = (
                    self.integer(from, "loop start")?,
                    self.integer(to, "loop end")?,
                );
                if b - a > MAX_LOOP {
                    return self.error(
                        "E-limit",
                        it.span,
                        format!("the loop runs {} times; the limit is {MAX_LOOP}", b - a),
                    );
                }
                for i in a..b {
                    self.scopes.push(BTreeMap::from([(
                        var.clone(),
                        Val::Q(Qty::plain(i as f64)),
                    )]));
                    self.items(body, f, false);
                    self.scopes.pop();
                    if self.aborted {
                        break;
                    }
                }
                Ok(())
            }
            ItemKind::If {
                cond,
                then,
                otherwise,
            } => {
                let c = self.eval(cond)?;
                let Val::Bool(b) = c else {
                    return self.error(
                        "E-type",
                        cond.span,
                        format!("the condition must be a boolean, found {}", c.type_name()),
                    );
                };
                self.scopes.push(BTreeMap::new());
                self.items(if b { then } else { otherwise }, f, false);
                self.scopes.pop();
                Ok(())
            }
        }
    }

    fn board_item(&mut self, props: &[Prop], span: Span) -> R<()> {
        if self.board.is_some() {
            return self.error("E-duplicate", span, "the design already has a board");
        }
        let m = self.props(props, &["outline", "layers"], "board");
        let outline = match m.get("outline") {
            Some(p) => self.polygon(&p.value, "board outline")?,
            None => Vec::new(),
        };
        if outline.len() < 3 {
            return self.error("E-property", span, "the board needs an `outline` of at least 3 points, e.g. rect(0mm, 0mm, 50mm, 30mm)");
        }
        let layers = match m.get("layers") {
            Some(p) => {
                let n = self.integer(&p.value, "layers")?;
                if !(2..=32).contains(&n) || n % 2 != 0 {
                    return self.error(
                        "E-value",
                        p.value.span,
                        "layers must be an even number from 2 to 32",
                    );
                }
                Some(n as u8)
            }
            None => None,
        };
        self.board = Some((BoardIr { outline, layers }, span));
        Ok(())
    }

    fn component(&mut self, name: &NameRef, props: &[Prop], f: &Frame, _span: Span) -> R<()> {
        let local = self.local_name(name)?;
        let reference = format!("{}{local}", f.prefix);
        if self.comps.contains_key(&reference) {
            return self.error(
                "E-duplicate",
                name.span,
                format!("component “{reference}” is already defined"),
            );
        }
        let m = self.props(props, &[], "component");
        let (mut value, mut footprint, mut symbol) = (String::new(), String::new(), String::new());
        let mut properties = BTreeMap::new();
        for (k, p) in &m {
            let v = match self.eval(&p.value)? {
                Val::Str(s) => s,
                Val::Q(q) => format_qty(q),
                Val::Bool(b) => b.to_string(),
                o => {
                    return self.error(
                        "E-type",
                        p.value.span,
                        format!(
                            "property “{k}” needs text or a number, found {}",
                            o.type_name()
                        ),
                    )
                }
            };
            match k.as_str() {
                "value" => value = v,
                "footprint" => footprint = v,
                "symbol" => symbol = v,
                _ => {
                    properties.insert(k.clone(), v);
                }
            }
        }
        let prefix_letters: String = local
            .chars()
            .take_while(|c| c.is_ascii_alphabetic())
            .collect();
        if symbol.is_empty() {
            match buturecad_eda_core::ir_apply::default_symbol(&local) {
                Some(s) => symbol = s.to_owned(),
                None => {
                    return self.error(
                        "E-symbol",
                        name.span,
                        format!("component “{local}” needs a `symbol` property (no default symbol for prefix “{prefix_letters}”; defaults exist for R C L V I D LED)"),
                    )
                }
            }
        }
        let symbol_pins = builtin_pins(&symbol);
        if symbol.starts_with("Buture:") && symbol_pins.is_none() {
            return self.error(
                "E-symbol",
                name.span,
                format!("unknown built-in symbol “{symbol}”"),
            );
        }
        self.order.push(reference.clone());
        self.comps.insert(
            reference.clone(),
            CompEntry {
                has_footprint: !footprint.is_empty(),
                ir: ComponentIr {
                    reference,
                    value,
                    footprint,
                    symbol,
                    properties,
                },
                span: name.span,
                symbol_pins,
                connected: false,
                placed: false,
            },
        );
        Ok(())
    }

    /// Adds one endpoint (a pin or a net name) to the set `anchor`.
    fn endpoint(&mut self, p: &PinRef, f: &Frame, anchor: usize) -> R<()> {
        let id = match &p.pin {
            None => {
                let n = self.net_name(&p.name, f)?;
                self.nets.named(&n, p.span)
            }
            Some((part, psp)) => {
                let reference = self.comp_ref(&p.name, f)?;
                let pin = match part {
                    PinPart::Name(s) => s.clone(),
                    PinPart::Number(s) => s.clone(),
                    PinPart::Computed(e) => self.integer(e, "pin number")?.to_string(),
                };
                self.pins.push(PendingPin {
                    reference: reference.clone(),
                    pin: pin.clone(),
                    span: p.span.to(*psp),
                });
                self.nets.pin(&reference, &pin)
            }
        };
        if let Err((a, b)) = self.nets.union(anchor, id) {
            return self.error(
                "E-short",
                p.span,
                format!("this joins two different nets, “{a}” and “{b}” (a short circuit)"),
            );
        }
        Ok(())
    }

    fn use_module(
        &mut self,
        name: &NameRef,
        module: &str,
        args: &[Arg],
        f: &Frame,
        span: Span,
    ) -> R<()> {
        let Some((params, body)) = self.modules.get(module).cloned() else {
            let known: Vec<_> = self.modules.keys().cloned().collect();
            return self.error(
                "E-undefined",
                span,
                format!(
                    "unknown module “{module}” (defined: {})",
                    if known.is_empty() {
                        "none".into()
                    } else {
                        known.join(", ")
                    }
                ),
            );
        };
        if self.module_depth >= MAX_MODULE_DEPTH {
            return self.error(
                "E-limit",
                span,
                "modules nest too deeply (a module that uses itself?)",
            );
        }
        let inst = self.local_name(name)?;
        for a in args {
            if !params.iter().any(|p| p.name == a.name) {
                return self.error(
                    "E-property",
                    a.span,
                    format!(
                        "module {module} has no parameter “{}” (parameters: {})",
                        a.name,
                        params
                            .iter()
                            .map(|p| p.name.as_str())
                            .collect::<Vec<_>>()
                            .join(", ")
                    ),
                );
            }
        }
        let mut vars = BTreeMap::new();
        let mut ports = BTreeMap::new();
        for p in &params {
            let arg = args.iter().find(|a| a.name == p.name);
            if p.port {
                let Some(a) = arg else {
                    return self.error(
                        "E-argument",
                        span,
                        format!("module {module} needs the port “{}”", p.name),
                    );
                };
                let net = match &a.value.kind {
                    ExprKind::Ident(n) if self.lookup(n).is_none() => n.clone(),
                    ExprKind::Ident(_) | ExprKind::Str(_) => self.string(&a.value, "port net")?,
                    _ => {
                        return self.error(
                            "E-type",
                            a.value.span,
                            "a port is connected to a net name",
                        )
                    }
                };
                let nr = NameRef {
                    base: net,
                    index: None,
                    span: a.value.span,
                };
                ports.insert(p.name.clone(), self.net_name(&nr, f)?);
            } else {
                let v = match (arg, &p.default) {
                    (Some(a), _) => self.eval(&a.value)?,
                    (None, Some(d)) => self.eval(d)?,
                    (None, None) => {
                        return self.error(
                            "E-argument",
                            span,
                            format!("module {module} needs the argument “{}”", p.name),
                        )
                    }
                };
                vars.insert(p.name.clone(), v);
            }
        }
        let frame = Frame {
            prefix: format!("{}{inst}_", f.prefix),
            ports,
            module: Some(module.to_owned()),
        };
        self.module_depth += 1;
        self.scopes.push(vars);
        self.items(&body, &frame, false);
        self.scopes.pop();
        self.module_depth -= 1;
        Ok(())
    }

    // -- validation and result ---------------------------------------------------------------------

    fn finish(&mut self) {
        // Pins exist.
        let pins = std::mem::take(&mut self.pins);
        for p in &pins {
            match self.comps.get_mut(&p.reference) {
                None => self.diags.push(Diagnostic::error(
                    "E-undefined",
                    p.span,
                    format!("no component named “{}”", p.reference),
                )),
                Some(c) => {
                    c.connected = true;
                    if let Some(list) = &c.symbol_pins {
                        if !list.iter().any(|(n, nm)| *n == p.pin || *nm == p.pin) {
                            self.diags.push(Diagnostic::error(
                                "E-pin",
                                p.span,
                                format!(
                                    "{} ({}) has no pin “{}” (pins: {})",
                                    p.reference,
                                    c.ir.symbol,
                                    p.pin,
                                    list.iter()
                                        .map(|(n, nm)| if nm == "~" || nm == n {
                                            n.clone()
                                        } else {
                                            format!("{n}/{nm}")
                                        })
                                        .collect::<Vec<_>>()
                                        .join(", ")
                                ),
                            ));
                        }
                    }
                }
            }
        }
        let layers = self.board.as_ref().and_then(|(b, _)| b.layers).unwrap_or(2);
        let valid_layer = |l: &str| -> bool {
            l == "F.Cu"
                || l == "B.Cu"
                || l.strip_prefix("In")
                    .and_then(|r| r.strip_suffix(".Cu"))
                    .and_then(|n| n.parse::<u8>().ok())
                    .is_some_and(|n| n >= 1 && n <= layers.saturating_sub(2))
        };
        for (t, sp) in &self.tracks {
            if !valid_layer(&t.layer) {
                self.diags.push(Diagnostic::error("E-layer", *sp, format!("“{}” is not a copper layer of a {layers}-layer board (F.Cu, In1.Cu…, B.Cu)", t.layer)));
            }
            if t.points.len() < 2 {
                self.diags.push(Diagnostic::error(
                    "E-value",
                    *sp,
                    "a track needs at least two points",
                ));
            }
        }
        for (z, sp) in &self.zones {
            if !valid_layer(&z.layer) {
                self.diags.push(Diagnostic::error(
                    "E-layer",
                    *sp,
                    format!(
                        "“{}” is not a copper layer of a {layers}-layer board",
                        z.layer
                    ),
                ));
            }
            if z.outline.len() < 3 {
                self.diags.push(Diagnostic::error(
                    "E-value",
                    *sp,
                    "a zone outline needs at least 3 points",
                ));
            }
        }
        for (p, sp) in &self.placements {
            match self.comps.get_mut(&p.reference) {
                None => self.diags.push(Diagnostic::error(
                    "E-undefined",
                    *sp,
                    format!("place: no component named “{}”", p.reference),
                )),
                Some(c) => {
                    if c.placed {
                        self.diags.push(Diagnostic::error(
                            "E-duplicate",
                            *sp,
                            format!("{} is placed twice", p.reference),
                        ));
                    }
                    c.placed = true;
                    if p.rotation_deg.rem_euclid(90) != 0 {
                        self.diags.push(Diagnostic::error(
                            "E-value",
                            *sp,
                            format!("rotation {}° is not a multiple of 90°", p.rotation_deg),
                        ));
                    }
                    if !c.has_footprint {
                        self.diags.push(Diagnostic::error(
                            "E-footprint",
                            *sp,
                            format!(
                                "{} has no footprint, so it cannot be placed on the board",
                                p.reference
                            ),
                        ));
                    }
                }
            }
        }
        // Warnings.
        for r in &self.order {
            let c = &self.comps[r];
            if !c.connected {
                self.diags.push(Diagnostic::warning(
                    "W-unused",
                    c.span,
                    format!("{r} is not connected to anything"),
                ));
            }
            if self.board.is_some() {
                if !c.has_footprint {
                    self.diags.push(Diagnostic::warning(
                        "W-footprint",
                        c.span,
                        format!("{r} has no footprint and will not appear on the board"),
                    ));
                } else if !c.placed {
                    self.diags.push(Diagnostic::warning(
                        "W-unplaced",
                        c.span,
                        format!("{r} is not placed (it will sit at the board origin)"),
                    ));
                }
            }
        }
        let roots: Vec<usize> = (0..self.nets.parent.len()).collect();
        for i in roots {
            if self.nets.find(i) != i {
                continue;
            }
            if let Some((n, sp)) = self.nets.name[i].clone() {
                if self.nets.pins[i].len() == 1 {
                    self.diags.push(Diagnostic::warning(
                        "W-net",
                        sp,
                        format!("net {n} has only one pin"),
                    ));
                } else if self.nets.pins[i].is_empty() {
                    self.diags.push(Diagnostic::warning(
                        "W-net",
                        sp,
                        format!("net {n} has no pins"),
                    ));
                }
            }
        }
    }

    pub fn into_design(mut self) -> (Option<Design>, Vec<Diagnostic>) {
        let mut diags = std::mem::take(&mut self.diags);
        diags.sort_by_key(|d| (d.span.line, d.span.col));
        if diags
            .iter()
            .any(|d| d.severity == crate::diag::Severity::Error)
        {
            return (None, diags);
        }
        // Nets.
        let mut nets: Vec<NetIr> = Vec::new();
        for i in 0..self.nets.parent.len() {
            if self.nets.find(i) != i || self.nets.pins[i].is_empty() {
                continue;
            }
            let mut pins = self.nets.pins[i].clone();
            pins.sort();
            pins.dedup();
            let name = match &self.nets.name[i] {
                Some((n, _)) => n.clone(),
                None => format!("N_{}_{}", pins[0].0, pins[0].1),
            };
            nets.push(NetIr {
                name,
                pins: pins
                    .into_iter()
                    .map(|(reference, pin)| PinRefIr { reference, pin })
                    .collect(),
            });
        }
        nets.sort_by(|a, b| a.name.cmp(&b.name));
        // A pin joined to two nets would have been a short; one pin in two sets cannot happen.
        let mut rules = self.rules.clone();
        rules.classes = self.classes.clone();
        let design = Design {
            name: self.project.clone().unwrap_or_else(|| "design".into()),
            board: self.board.as_ref().map(|(b, _)| b.clone()),
            components: self
                .order
                .iter()
                .map(|r| self.comps[r].ir.clone())
                .collect(),
            nets,
            placements: self.placements.iter().map(|(p, _)| p.clone()).collect(),
            tracks: self.tracks.iter().map(|(t, _)| t.clone()).collect(),
            vias: self.vias.clone(),
            zones: self.zones.iter().map(|(z, _)| z.clone()).collect(),
            rules,
        };
        let _: BTreeSet<()> = BTreeSet::new();
        (Some(design), diags)
    }
}
