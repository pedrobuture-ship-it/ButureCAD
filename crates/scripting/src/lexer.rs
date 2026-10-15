//! Lexer: source text → tokens with positions. Never panics; bad characters become diagnostics.

use crate::diag::{Diagnostic, Span};

#[derive(Debug, Clone, PartialEq)]
pub enum Tok {
    Ident(String),
    /// Number text and the unit written right after it (`10mm` → `("10", Some("mm"))`).
    Num(f64, Option<String>),
    Str(String),
    LBrace,
    RBrace,
    LParen,
    RParen,
    LBracket,
    RBracket,
    Comma,
    Semi,
    Colon,
    Dot,
    DotDot,
    Arrow,
    Assign,
    Plus,
    Minus,
    Star,
    Slash,
    Percent,
    EqEq,
    NotEq,
    Le,
    Ge,
    Lt,
    Gt,
    AndAnd,
    OrOr,
    Bang,
    Eof,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Token {
    pub tok: Tok,
    pub span: Span,
}

pub fn lex(src: &str) -> (Vec<Token>, Vec<Diagnostic>) {
    let chars: Vec<char> = src.chars().collect();
    let mut out = Vec::new();
    let mut diags = Vec::new();
    let (mut i, mut line, mut col) = (0usize, 1u32, 1u32);
    macro_rules! adv {
        () => {{
            if chars[i] == '\n' {
                line += 1;
                col = 1;
            } else {
                col += 1;
            }
            i += 1;
        }};
    }
    while i < chars.len() {
        let c = chars[i];
        let (sl, sc) = (line, col);
        if c.is_whitespace() {
            adv!();
            continue;
        }
        // Comments: `//`, `#`, `/* */`.
        if c == '#' || (c == '/' && chars.get(i + 1) == Some(&'/')) {
            while i < chars.len() && chars[i] != '\n' {
                adv!();
            }
            continue;
        }
        if c == '/' && chars.get(i + 1) == Some(&'*') {
            adv!();
            adv!();
            let mut closed = false;
            while i < chars.len() {
                if chars[i] == '*' && chars.get(i + 1) == Some(&'/') {
                    adv!();
                    adv!();
                    closed = true;
                    break;
                }
                adv!();
            }
            if !closed {
                diags.push(Diagnostic::error(
                    "E-syntax",
                    Span::new(sl, sc, 2),
                    "unterminated block comment",
                ));
            }
            continue;
        }
        let push = |tok: Tok, len: u32, out: &mut Vec<Token>| {
            out.push(Token {
                tok,
                span: Span::new(sl, sc, len),
            })
        };
        if c.is_alphabetic() || c == '_' {
            let st = i;
            while i < chars.len() && (chars[i].is_alphanumeric() || chars[i] == '_') {
                adv!();
            }
            let s: String = chars[st..i].iter().collect();
            let n = s.chars().count() as u32;
            push(Tok::Ident(s), n, &mut out);
            continue;
        }
        if c.is_ascii_digit() {
            let st = i;
            while i < chars.len() && chars[i].is_ascii_digit() {
                adv!();
            }
            if i < chars.len()
                && chars[i] == '.'
                && chars.get(i + 1).is_some_and(|n| n.is_ascii_digit())
            {
                adv!();
                while i < chars.len() && chars[i].is_ascii_digit() {
                    adv!();
                }
            }
            if i < chars.len() && (chars[i] == 'e' || chars[i] == 'E') {
                let mut j = i + 1;
                if chars.get(j).is_some_and(|n| *n == '+' || *n == '-') {
                    j += 1;
                }
                if chars.get(j).is_some_and(|n| n.is_ascii_digit()) {
                    while i < j {
                        adv!();
                    }
                    while i < chars.len() && chars[i].is_ascii_digit() {
                        adv!();
                    }
                }
            }
            let text: String = chars[st..i].iter().collect();
            let ust = i;
            while i < chars.len()
                && (chars[i].is_alphabetic() || chars[i] == 'µ' || chars[i] == 'Ω')
            {
                adv!();
            }
            let unit: String = chars[ust..i].iter().collect();
            let len = (i - st) as u32;
            match text.parse::<f64>() {
                Ok(v) if v.is_finite() => push(
                    Tok::Num(v, (!unit.is_empty()).then_some(unit)),
                    len,
                    &mut out,
                ),
                _ => diags.push(Diagnostic::error(
                    "E-syntax",
                    Span::new(sl, sc, len),
                    format!("bad number “{text}”"),
                )),
            }
            continue;
        }
        if c == '"' {
            adv!();
            let mut s = String::new();
            let mut closed = false;
            while i < chars.len() {
                match chars[i] {
                    '"' => {
                        adv!();
                        closed = true;
                        break;
                    }
                    '\\' => {
                        adv!();
                        match chars.get(i) {
                            Some('n') => s.push('\n'),
                            Some('"') => s.push('"'),
                            Some('\\') => s.push('\\'),
                            Some(o) => {
                                diags.push(Diagnostic::error(
                                    "E-syntax",
                                    Span::new(line, col, 1),
                                    format!("unknown escape \\{o}"),
                                ));
                            }
                            None => break,
                        }
                        if i < chars.len() {
                            adv!();
                        }
                    }
                    '\n' => break,
                    ch => {
                        s.push(ch);
                        adv!();
                    }
                }
            }
            if closed {
                let len = if line == sl { col - sc } else { 1 };
                out.push(Token {
                    tok: Tok::Str(s),
                    span: Span::new(sl, sc, len),
                });
            } else {
                diags.push(Diagnostic::error(
                    "E-syntax",
                    Span::new(sl, sc, 1),
                    "unterminated string",
                ));
            }
            continue;
        }
        let two = chars.get(i + 1).copied();
        let (tok, n) = match (c, two) {
            ('-', Some('>')) => (Tok::Arrow, 2),
            ('.', Some('.')) => (Tok::DotDot, 2),
            ('=', Some('=')) => (Tok::EqEq, 2),
            ('!', Some('=')) => (Tok::NotEq, 2),
            ('<', Some('=')) => (Tok::Le, 2),
            ('>', Some('=')) => (Tok::Ge, 2),
            ('&', Some('&')) => (Tok::AndAnd, 2),
            ('|', Some('|')) => (Tok::OrOr, 2),
            ('{', _) => (Tok::LBrace, 1),
            ('}', _) => (Tok::RBrace, 1),
            ('(', _) => (Tok::LParen, 1),
            (')', _) => (Tok::RParen, 1),
            ('[', _) => (Tok::LBracket, 1),
            (']', _) => (Tok::RBracket, 1),
            (',', _) => (Tok::Comma, 1),
            (';', _) => (Tok::Semi, 1),
            (':', _) => (Tok::Colon, 1),
            ('.', _) => (Tok::Dot, 1),
            ('=', _) => (Tok::Assign, 1),
            ('+', _) => (Tok::Plus, 1),
            ('-', _) => (Tok::Minus, 1),
            ('*', _) => (Tok::Star, 1),
            ('/', _) => (Tok::Slash, 1),
            ('%', _) => (Tok::Percent, 1),
            ('<', _) => (Tok::Lt, 1),
            ('>', _) => (Tok::Gt, 1),
            ('!', _) => (Tok::Bang, 1),
            _ => {
                diags.push(Diagnostic::error(
                    "E-syntax",
                    Span::new(sl, sc, 1),
                    format!("unexpected character “{c}”"),
                ));
                adv!();
                continue;
            }
        };
        for _ in 0..n {
            adv!();
        }
        out.push(Token {
            tok,
            span: Span::new(sl, sc, n as u32),
        });
    }
    out.push(Token {
        tok: Tok::Eof,
        span: Span::new(line, col, 1),
    });
    (out, diags)
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;

    fn toks(s: &str) -> Vec<Tok> {
        lex(s).0.into_iter().map(|t| t.tok).collect()
    }

    #[test]
    fn numbers_units_ranges_and_pin_references() {
        assert_eq!(
            toks("10mm 2.54mm 1..8 C1.2"),
            [
                Tok::Num(10.0, Some("mm".into())),
                Tok::Num(2.54, Some("mm".into())),
                Tok::Num(1.0, None),
                Tok::DotDot,
                Tok::Num(8.0, None),
                Tok::Ident("C1".into()),
                Tok::Dot,
                Tok::Num(2.0, None),
                Tok::Eof
            ]
        );
        assert_eq!(
            toks("1e3 5V 100kOhm"),
            [
                Tok::Num(1000.0, None),
                Tok::Num(5.0, Some("V".into())),
                Tok::Num(100.0, Some("kOhm".into())),
                Tok::Eof
            ]
        );
    }

    #[test]
    fn strings_comments_and_operators() {
        assert_eq!(
            toks("\"a\\\"b\" // x\n# y\n/* z */ -> == != <= && !"),
            [
                Tok::Str("a\"b".into()),
                Tok::Arrow,
                Tok::EqEq,
                Tok::NotEq,
                Tok::Le,
                Tok::AndAnd,
                Tok::Bang,
                Tok::Eof
            ]
        );
    }

    #[test]
    fn positions_and_errors() {
        let (t, d) = lex("ab\n  cd");
        assert_eq!((t[1].span.line, t[1].span.col), (2, 3));
        assert!(d.is_empty());
        let (_, d) = lex("a $ \"open\n/* never");
        assert_eq!(d.len(), 3, "{d:?}");
        assert_eq!((d[0].span.line, d[0].span.col), (1, 3));
        assert!(
            lex("\u{0}\u{7f}é€").1.len() >= 2,
            "never panics on odd input"
        );
    }
}
