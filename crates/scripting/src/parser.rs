//! Recursive-descent parser. Statements start with a keyword, so no terminators are needed;
//! after a syntax error the parser skips to the next statement keyword and goes on, so one run
//! reports several errors.

use crate::ast::*;
use crate::diag::{Diagnostic, Span};
use crate::lexer::{Tok, Token};

const KEYWORDS: [&str; 17] = [
    "project",
    "board",
    "component",
    "net",
    "connect",
    "place",
    "track",
    "via",
    "zone",
    "rule",
    "class",
    "let",
    "param",
    "fn",
    "module",
    "use",
    "for",
];

fn is_stmt_start(t: &Tok) -> bool {
    matches!(t, Tok::Ident(s) if KEYWORDS.contains(&s.as_str()) || s == "if")
}

struct Parser {
    toks: Vec<Token>,
    pos: usize,
    diags: Vec<Diagnostic>,
    /// Current recursion depth of nested constructs.
    nest: u32,
}

/// Nested parentheses / blocks / unary chains allowed (the parser is recursive).
const MAX_NEST: u32 = 100;
/// Height of an expression tree allowed (evaluation recurses over it).
const MAX_EXPR_DEPTH: u32 = 400;

type R<T> = Result<T, ()>;

impl Parser {
    fn peek(&self) -> &Tok {
        &self.toks[self.pos].tok
    }
    fn peek_at(&self, n: usize) -> &Tok {
        &self.toks[(self.pos + n).min(self.toks.len() - 1)].tok
    }
    fn span(&self) -> Span {
        self.toks[self.pos].span
    }
    fn prev_span(&self) -> Span {
        self.toks[self.pos.saturating_sub(1)].span
    }
    fn bump(&mut self) -> Token {
        let t = self.toks[self.pos].clone();
        if self.pos + 1 < self.toks.len() {
            self.pos += 1;
        }
        t
    }
    fn err<T>(&mut self, span: Span, msg: impl Into<String>) -> R<T> {
        self.diags.push(Diagnostic::error("E-syntax", span, msg));
        Err(())
    }
    fn describe(t: &Tok) -> String {
        match t {
            Tok::Ident(s) => format!("“{s}”"),
            Tok::Num(v, u) => format!("number {v}{}", u.clone().unwrap_or_default()),
            Tok::Str(_) => "a string".into(),
            Tok::Eof => "the end of the file".into(),
            other => format!("{other:?}"),
        }
    }
    fn expect(&mut self, want: Tok, what: &str) -> R<Token> {
        if *self.peek() == want {
            Ok(self.bump())
        } else {
            let sp = self.span();
            let got = Self::describe(self.peek());
            self.err(sp, format!("expected {what}, found {got}"))
        }
    }
    fn eat(&mut self, t: &Tok) -> bool {
        if self.peek() == t {
            self.bump();
            true
        } else {
            false
        }
    }
    fn eat_kw(&mut self, kw: &str) -> bool {
        if matches!(self.peek(), Tok::Ident(s) if s == kw) {
            self.bump();
            true
        } else {
            false
        }
    }
    fn expect_ident(&mut self, what: &str) -> R<(String, Span)> {
        match self.peek().clone() {
            Tok::Ident(s) => {
                let sp = self.span();
                self.bump();
                Ok((s, sp))
            }
            other => {
                let sp = self.span();
                self.err(
                    sp,
                    format!("expected {what}, found {}", Self::describe(&other)),
                )
            }
        }
    }

    // -- expressions -----------------------------------------------------------------------------

    /// Runs `f` one nesting level deeper; refuses to go past [`MAX_NEST`].
    fn nested<T>(&mut self, f: impl FnOnce(&mut Self) -> R<T>) -> R<T> {
        if self.nest >= MAX_NEST {
            let sp = self.span();
            return self.err(
                sp,
                format!("this is nested more than {MAX_NEST} levels deep"),
            );
        }
        self.nest += 1;
        let r = f(self);
        self.nest -= 1;
        r
    }

    fn mk(&mut self, kind: ExprKind, span: Span) -> R<Expr> {
        let e = Expr::new(kind, span);
        if e.depth > MAX_EXPR_DEPTH {
            return self.err(
                span,
                format!("this expression is more than {MAX_EXPR_DEPTH} levels deep"),
            );
        }
        Ok(e)
    }

    fn expr(&mut self) -> R<Expr> {
        self.nested(|p| p.binary(0))
    }

    fn binary(&mut self, level: u8) -> R<Expr> {
        const LEVELS: [&[(Tok, Op)]; 6] = [
            &[(Tok::OrOr, Op::Or)],
            &[(Tok::AndAnd, Op::And)],
            &[(Tok::EqEq, Op::Eq), (Tok::NotEq, Op::Ne)],
            &[
                (Tok::Lt, Op::Lt),
                (Tok::Le, Op::Le),
                (Tok::Gt, Op::Gt),
                (Tok::Ge, Op::Ge),
            ],
            &[(Tok::Plus, Op::Add), (Tok::Minus, Op::Sub)],
            &[
                (Tok::Star, Op::Mul),
                (Tok::Slash, Op::Div),
                (Tok::Percent, Op::Rem),
            ],
        ];
        if level as usize == LEVELS.len() {
            return self.unary();
        }
        let mut lhs = self.binary(level + 1)?;
        loop {
            let Some((_, op)) = LEVELS[level as usize]
                .iter()
                .find(|(t, _)| t == self.peek())
            else {
                return Ok(lhs);
            };
            let op = *op;
            self.bump();
            let rhs = self.binary(level + 1)?;
            let span = lhs.span.to(rhs.span);
            lhs = self.mk(ExprKind::Binary(op, Box::new(lhs), Box::new(rhs)), span)?;
        }
    }

    fn unary(&mut self) -> R<Expr> {
        let sp = self.span();
        let op = match self.peek() {
            Tok::Minus => Op::Neg,
            Tok::Bang => Op::Not,
            _ => return self.primary(),
        };
        self.bump();
        let e = self.nested(|p| p.unary())?;
        let span = sp.to(e.span);
        self.mk(ExprKind::Unary(op, Box::new(e)), span)
    }

    fn primary(&mut self) -> R<Expr> {
        let sp = self.span();
        match self.peek().clone() {
            Tok::Num(v, u) => {
                self.bump();
                self.mk(ExprKind::Num(v, u), sp)
            }
            Tok::Str(s) => {
                self.bump();
                self.mk(ExprKind::Str(s), sp)
            }
            Tok::Ident(s) if s == "true" || s == "false" => {
                self.bump();
                self.mk(ExprKind::Bool(s == "true"), sp)
            }
            Tok::Ident(s) => {
                self.bump();
                if *self.peek() == Tok::LParen {
                    self.bump();
                    let mut args = Vec::new();
                    while *self.peek() != Tok::RParen {
                        args.push(self.expr()?);
                        if !self.eat(&Tok::Comma) {
                            break;
                        }
                    }
                    let end = self.expect(Tok::RParen, "“)”")?;
                    self.mk(ExprKind::Call(s, args), sp.to(end.span))
                } else {
                    self.mk(ExprKind::Ident(s), sp)
                }
            }
            Tok::LParen => {
                self.bump();
                let first = self.expr()?;
                if self.eat(&Tok::Comma) {
                    let mut v = vec![first];
                    while *self.peek() != Tok::RParen {
                        v.push(self.expr()?);
                        if !self.eat(&Tok::Comma) {
                            break;
                        }
                    }
                    let end = self.expect(Tok::RParen, "“)”")?;
                    self.mk(ExprKind::Tuple(v), sp.to(end.span))
                } else {
                    self.expect(Tok::RParen, "“)”")?;
                    Ok(first)
                }
            }
            Tok::LBracket => {
                self.bump();
                let mut v = Vec::new();
                while *self.peek() != Tok::RBracket {
                    v.push(self.expr()?);
                    if !self.eat(&Tok::Comma) {
                        break;
                    }
                }
                let end = self.expect(Tok::RBracket, "“]”")?;
                self.mk(ExprKind::List(v), sp.to(end.span))
            }
            other => self.err(
                sp,
                format!("expected an expression, found {}", Self::describe(&other)),
            ),
        }
    }

    // -- names and pins --------------------------------------------------------------------------

    fn name_ref(&mut self, what: &str) -> R<NameRef> {
        let (base, sp) = self.expect_ident(what)?;
        let mut span = sp;
        let index = if *self.peek() == Tok::LBracket {
            self.bump();
            let e = self.expr()?;
            let end = self.expect(Tok::RBracket, "“]”")?;
            span = sp.to(end.span);
            Some(e)
        } else {
            None
        };
        Ok(NameRef { base, index, span })
    }

    fn pin_ref(&mut self) -> R<PinRef> {
        // A quoted name is always a net (`"+3V3"`).
        if let Tok::Str(s) = self.peek().clone() {
            let sp = self.span();
            self.bump();
            return Ok(PinRef {
                name: NameRef {
                    base: s,
                    index: None,
                    span: sp,
                },
                pin: None,
                span: sp,
            });
        }
        let name = self.name_ref("a component or net name")?;
        let mut span = name.span;
        let pin = if *self.peek() == Tok::Dot {
            self.bump();
            let psp = self.span();
            let part = match self.peek().clone() {
                Tok::Ident(s) => {
                    self.bump();
                    PinPart::Name(s)
                }
                Tok::Num(v, None) if v.fract() == 0.0 && v >= 0.0 => {
                    self.bump();
                    PinPart::Number((v as u64).to_string())
                }
                Tok::LParen => {
                    self.bump();
                    let e = self.expr()?;
                    self.expect(Tok::RParen, "“)”")?;
                    PinPart::Computed(e)
                }
                other => {
                    return self.err(
                        psp,
                        format!(
                            "expected a pin name or number after “.”, found {}",
                            Self::describe(&other)
                        ),
                    )
                }
            };
            span = span.to(self.prev_span());
            Some((part, psp))
        } else {
            None
        };
        Ok(PinRef { name, pin, span })
    }

    fn layer_name(&mut self) -> R<String> {
        if let Tok::Str(s) = self.peek().clone() {
            self.bump();
            return Ok(s);
        }
        let (mut s, _) = self.expect_ident("a layer name such as F.Cu")?;
        while *self.peek() == Tok::Dot {
            self.bump();
            let (p, _) = self.expect_ident("a layer name part")?;
            s.push('.');
            s.push_str(&p);
        }
        Ok(s)
    }

    // -- statements ------------------------------------------------------------------------------

    fn props(&mut self) -> R<Vec<Prop>> {
        self.expect(Tok::LBrace, "“{”")?;
        let mut v = Vec::new();
        while *self.peek() != Tok::RBrace && *self.peek() != Tok::Eof {
            let (key, key_span) = self.expect_ident("a property name")?;
            self.expect(Tok::Assign, "“=”")?;
            let value = self.expr()?;
            v.push(Prop {
                key,
                key_span,
                value,
            });
            self.eat(&Tok::Comma);
            self.eat(&Tok::Semi);
        }
        self.expect(Tok::RBrace, "“}”")?;
        Ok(v)
    }

    fn block(&mut self) -> R<Vec<Item>> {
        self.nested(Self::block_inner)
    }

    fn block_inner(&mut self) -> R<Vec<Item>> {
        self.expect(Tok::LBrace, "“{”")?;
        let mut items = Vec::new();
        while *self.peek() != Tok::RBrace && *self.peek() != Tok::Eof {
            if let Some(i) = self.item() {
                items.push(i);
            }
        }
        self.expect(Tok::RBrace, "“}”")?;
        Ok(items)
    }

    fn item(&mut self) -> Option<Item> {
        let start = self.pos;
        match self.item_inner() {
            Ok(i) => Some(i),
            Err(()) => {
                // Skip to the next statement keyword (or a closing brace) and carry on.
                if self.pos == start {
                    self.bump();
                }
                let mut depth = 0i32;
                while *self.peek() != Tok::Eof {
                    match self.peek() {
                        Tok::LBrace => depth += 1,
                        Tok::RBrace if depth == 0 => break,
                        Tok::RBrace => depth -= 1,
                        t if depth == 0 && is_stmt_start(t) => break,
                        _ => {}
                    }
                    self.bump();
                }
                None
            }
        }
    }

    fn item_inner(&mut self) -> R<Item> {
        let sp = self.span();
        let kw = match self.peek().clone() {
            Tok::Ident(s) => s,
            other => {
                return self.err(
                    sp,
                    format!(
                        "expected a statement (component, net, connect, …), found {}",
                        Self::describe(&other)
                    ),
                )
            }
        };
        if !KEYWORDS.contains(&kw.as_str()) && kw != "if" {
            return self.err(sp, format!("unknown statement “{kw}” (expected component, net, connect, place, track, via, zone, rule, board, let, for, if, …)"));
        }
        self.bump();
        let kind = match kw.as_str() {
            "project" => ItemKind::Project(self.expr()?),
            "board" => ItemKind::Board(self.props()?),
            "component" => {
                let name = self.name_ref("a component name")?;
                ItemKind::Component {
                    name,
                    props: self.props()?,
                }
            }
            "net" => {
                let name = self.name_ref("a net name")?;
                let mut pins = Vec::new();
                if self.eat(&Tok::LBrace) {
                    while *self.peek() != Tok::RBrace && *self.peek() != Tok::Eof {
                        pins.push(self.pin_ref()?);
                        self.eat(&Tok::Comma);
                    }
                    self.expect(Tok::RBrace, "“}”")?;
                }
                ItemKind::Net { name, pins }
            }
            "connect" => {
                let mut groups = Vec::new();
                loop {
                    let mut g = vec![self.pin_ref()?];
                    while self.eat(&Tok::Comma) {
                        g.push(self.pin_ref()?);
                    }
                    groups.push(g);
                    if !self.eat(&Tok::Arrow) {
                        break;
                    }
                }
                if groups.len() < 2 {
                    let sp = self.span();
                    return self.err(sp, "connect needs at least two endpoints joined by “->”");
                }
                ItemKind::Connect(groups)
            }
            "place" => {
                let target = self.name_ref("a component name")?;
                if !self.eat_kw("at") {
                    let sp = self.span();
                    return self.err(sp, "expected “at (x, y)” after the component name");
                }
                let at = self.expr()?;
                let mut rotate = None;
                let mut side = None;
                loop {
                    if self.eat_kw("rotate") {
                        rotate = Some(self.expr()?);
                    } else if self.eat_kw("side") {
                        side = Some(self.expect_ident("top or bottom")?);
                    } else {
                        break;
                    }
                }
                ItemKind::Place {
                    target,
                    at,
                    rotate,
                    side,
                }
            }
            "track" => {
                let net = self.name_ref("a net name")?;
                if !self.eat_kw("on") {
                    let sp = self.span();
                    return self.err(sp, "expected “on <layer>”");
                }
                let layer = self.layer_name()?;
                if !self.eat_kw("width") {
                    let sp = self.span();
                    return self.err(sp, "expected “width <length>”");
                }
                let width = self.expr()?;
                if !self.eat_kw("path") {
                    let sp = self.span();
                    return self.err(sp, "expected “path (x, y) -> (x, y)”");
                }
                let mut path = vec![self.expr()?];
                while self.eat(&Tok::Arrow) {
                    path.push(self.expr()?);
                }
                ItemKind::Track {
                    net,
                    layer,
                    width,
                    path,
                }
            }
            "via" => {
                let net = self.name_ref("a net name")?;
                if !self.eat_kw("at") {
                    let sp = self.span();
                    return self.err(sp, "expected “at (x, y)”");
                }
                let at = self.expr()?;
                let (mut diameter, mut drill) = (None, None);
                loop {
                    if self.eat_kw("diameter") {
                        diameter = Some(self.expr()?);
                    } else if self.eat_kw("drill") {
                        drill = Some(self.expr()?);
                    } else {
                        break;
                    }
                }
                ItemKind::Via {
                    net,
                    at,
                    diameter,
                    drill,
                }
            }
            "zone" => {
                let net = self.name_ref("a net name")?;
                if !self.eat_kw("on") {
                    let sp = self.span();
                    return self.err(sp, "expected “on <layer>”");
                }
                let layer = self.layer_name()?;
                ItemKind::Zone {
                    net,
                    layer,
                    props: self.props()?,
                }
            }
            "rule" => ItemKind::Rule(self.props()?),
            "class" => {
                let name = if let Tok::Str(s) = self.peek().clone() {
                    self.bump();
                    s
                } else {
                    self.expect_ident("a class name")?.0
                };
                ItemKind::Class {
                    name,
                    props: self.props()?,
                }
            }
            "let" | "param" => {
                let (name, _) = self.expect_ident("a name")?;
                self.expect(Tok::Assign, "“=”")?;
                let value = self.expr()?;
                if kw == "let" {
                    ItemKind::Let { name, value }
                } else {
                    ItemKind::ParamDecl { name, value }
                }
            }
            "fn" => {
                let (name, _) = self.expect_ident("a function name")?;
                self.expect(Tok::LParen, "“(”")?;
                let mut params = Vec::new();
                while *self.peek() != Tok::RParen {
                    params.push(self.expect_ident("a parameter name")?.0);
                    if !self.eat(&Tok::Comma) {
                        break;
                    }
                }
                self.expect(Tok::RParen, "“)”")?;
                self.expect(Tok::Assign, "“=” followed by the function's expression")?;
                ItemKind::Fn {
                    name,
                    params,
                    body: self.expr()?,
                }
            }
            "module" => {
                let (name, _) = self.expect_ident("a module name")?;
                self.expect(Tok::LParen, "“(”")?;
                let mut params = Vec::new();
                while *self.peek() != Tok::RParen {
                    let port = self.eat_kw("port");
                    let (pname, span) = self.expect_ident("a parameter name")?;
                    let default = if self.eat(&Tok::Assign) {
                        Some(self.expr()?)
                    } else {
                        None
                    };
                    params.push(Param {
                        name: pname,
                        span,
                        port,
                        default,
                    });
                    if !self.eat(&Tok::Comma) {
                        break;
                    }
                }
                self.expect(Tok::RParen, "“)”")?;
                ItemKind::Module {
                    name,
                    params,
                    body: self.block()?,
                }
            }
            "use" => {
                let name = self.name_ref("an instance name")?;
                self.expect(Tok::Assign, "“=”")?;
                let (module, _) = self.expect_ident("a module name")?;
                self.expect(Tok::LParen, "“(”")?;
                let mut args = Vec::new();
                while *self.peek() != Tok::RParen {
                    let (aname, span) = self.expect_ident("an argument name")?;
                    self.expect(Tok::Assign, "“=”")?;
                    args.push(Arg {
                        name: aname,
                        span,
                        value: self.expr()?,
                    });
                    if !self.eat(&Tok::Comma) {
                        break;
                    }
                }
                self.expect(Tok::RParen, "“)”")?;
                ItemKind::Use { name, module, args }
            }
            "for" => {
                let (var, _) = self.expect_ident("a loop variable")?;
                if !self.eat_kw("in") {
                    let sp = self.span();
                    return self.err(sp, "expected “in <from>..<to>”");
                }
                let from = self.expr()?;
                self.expect(Tok::DotDot, "“..”")?;
                let to = self.expr()?;
                ItemKind::For {
                    var,
                    from,
                    to,
                    body: self.block()?,
                }
            }
            _ => {
                // `if`
                let cond = self.expr()?;
                let then = self.block()?;
                let otherwise = if self.eat_kw("else") {
                    if matches!(self.peek(), Tok::Ident(s) if s == "if") {
                        let sp = self.span();
                        self.bump();
                        let inner = self.item_if(sp)?;
                        vec![inner]
                    } else {
                        self.block()?
                    }
                } else {
                    Vec::new()
                };
                ItemKind::If {
                    cond,
                    then,
                    otherwise,
                }
            }
        };
        Ok(Item {
            kind,
            span: sp.to(self.prev_span()),
        })
    }

    /// The `if` after `else` (the keyword is already consumed).
    fn item_if(&mut self, sp: Span) -> R<Item> {
        let cond = self.expr()?;
        let then = self.block()?;
        let otherwise = if self.eat_kw("else") {
            if matches!(self.peek(), Tok::Ident(s) if s == "if") {
                let sp2 = self.span();
                self.bump();
                vec![self.item_if(sp2)?]
            } else {
                self.block()?
            }
        } else {
            Vec::new()
        };
        Ok(Item {
            kind: ItemKind::If {
                cond,
                then,
                otherwise,
            },
            span: sp.to(self.prev_span()),
        })
    }
}

/// Parses a single expression (used for `--param name=value`).
pub fn parse_expr(tokens: Vec<Token>) -> Result<Expr, Vec<Diagnostic>> {
    let mut p = Parser {
        toks: tokens,
        pos: 0,
        diags: Vec::new(),
        nest: 0,
    };
    match p.expr() {
        Ok(e) if *p.peek() == Tok::Eof => Ok(e),
        Ok(_) => {
            let sp = p.span();
            p.diags.push(Diagnostic::error(
                "E-syntax",
                sp,
                "unexpected text after the value",
            ));
            Err(p.diags)
        }
        Err(()) => Err(p.diags),
    }
}

pub fn parse(tokens: Vec<Token>) -> (Program, Vec<Diagnostic>) {
    let mut p = Parser {
        toks: tokens,
        pos: 0,
        diags: Vec::new(),
        nest: 0,
    };
    let mut items = Vec::new();
    while *p.peek() != Tok::Eof {
        if let Some(i) = p.item() {
            items.push(i);
        }
        if *p.peek() == Tok::RBrace {
            let sp = p.span();
            p.diags
                .push(Diagnostic::error("E-syntax", sp, "unmatched “}”"));
            p.bump();
        }
    }
    let _ = p.peek_at(0);
    (Program { items }, p.diags)
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::panic)]
mod tests {
    use super::*;
    use crate::lexer::lex;

    fn parse_src(s: &str) -> (Program, Vec<Diagnostic>) {
        let (t, mut d) = lex(s);
        let (p, d2) = parse(t);
        d.extend(d2);
        (p, d)
    }

    #[test]
    fn the_conceptual_example_parses() {
        let (p, d) = parse_src(
            "component MCU { part = \"STM32F411\" }\ncomponent C1 { value = \"100nF\"\n footprint = \"0402\" }\nconnect MCU.VDD -> C1.1\nconnect C1.2 -> GND\n",
        );
        assert!(d.is_empty(), "{d:?}");
        assert_eq!(p.items.len(), 4);
        let ItemKind::Connect(g) = &p.items[2].kind else {
            panic!()
        };
        assert_eq!(g.len(), 2);
        assert!(matches!(&g[1][0].pin, Some((PinPart::Number(n), _)) if n == "1"));
        let ItemKind::Connect(g) = &p.items[3].kind else {
            panic!()
        };
        assert!(g[1][0].pin.is_none(), "bare GND is a net");
    }

    #[test]
    fn expressions_precedence_and_every_statement_form() {
        let src = r#"
project "demo"
board { outline = rect(0mm, 0mm, 50mm, 30mm) layers = 2 }
param pitch = 2.54mm
let n = 8
fn col(i) = i * pitch + 1mm
module Decouple(port VDD, port GND, value = "100nF") {
  component C { value = value footprint = "0402" }
  connect VDD -> C.1
  connect C.2 -> GND
}
use d1 = Decouple(VDD = VCC, GND = GND)
for i in 0..n {
  component LED[i] { symbol = "Buture:LED" }
  place LED[i] at (col(i), 20mm) rotate 90 side top
  if i % 2 == 0 && i > 0 { net EVEN[i] { LED[i].A, LED[i].K } } else if i == 1 { } else { }
}
track VCC on F.Cu width 0.25mm path (1mm, 1mm) -> (10mm, 1mm) -> (10mm, 5mm)
via VCC at (5mm, 5mm) diameter 0.8mm drill 0.4mm
zone GND on "B.Cu" { outline = rect(0mm,0mm,50mm,30mm) clearance = 0.3mm }
rule { clearance = 0.2mm }
class Power { width = 0.5mm nets = [VCC, GND] }
"#;
        let (p, d) = parse_src(src);
        assert!(d.is_empty(), "{d:?}");
        assert_eq!(p.items.len(), 13);
        let (e, _) = {
            let (t, _) = lex("1 + 2 * 3 - 4 < 5 || !x");
            let mut pr = Parser {
                toks: t,
                pos: 0,
                diags: vec![],
                nest: 0,
            };
            (pr.expr().unwrap(), ())
        };
        assert!(matches!(e.kind, ExprKind::Binary(Op::Or, _, _)));
    }

    #[test]
    fn syntax_errors_have_positions_and_the_parser_recovers() {
        let (p, d) = parse_src(
            "component { }\nnet A { U1.1 }\nconnect A.1\nplace R1 (1mm, 1mm)\nbogus 1\nnet OK\n",
        );
        assert_eq!(d.len(), 3, "{d:?}");
        assert_eq!((d[0].span.line, d[0].span.col), (1, 11));
        assert!(d[0].message.contains("component name"));
        assert!(d[1].message.contains("at least two endpoints"));
        assert!(d[2].message.contains("at"));
        assert!(parse_src("bogus 1").1[0]
            .message
            .contains("unknown statement"));
        assert_eq!(
            p.items.len(),
            2,
            "the valid statements survive: net A and net OK"
        );
        assert!(parse_src("net A { U1.1").1.len() == 1);
        assert!(!parse_src("}").1.is_empty());
        // Garbage never panics or loops.
        for junk in [
            "((((",
            "component",
            "for in in { }",
            "fn f( = ",
            "-> -> ->",
            "\"unterminated",
        ] {
            let _ = parse_src(junk);
        }
    }
}
