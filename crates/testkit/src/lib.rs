//! Deterministic mutation fuzzing for the parsers, usable in plain `cargo test` (no nightly, no
//! external fuzzer). Inputs are valid files that are damaged in the ways real files get damaged:
//! flipped and replaced bytes, truncation, duplicated and removed chunks, inserted junk, swapped
//! tokens. The same seed always produces the same inputs, so a failure is reproducible.
//!
//! `FUZZ_ITERS=<n>` (environment) scales the number of cases per target; the default keeps the
//! test-suite fast. Long runs: `FUZZ_ITERS=200000 cargo test --release -- fuzz`.

/// xorshift64*.
#[derive(Debug, Clone)]
pub struct Rng(u64);

impl Rng {
    pub fn new(seed: u64) -> Self {
        Self(seed.wrapping_mul(0x9E37_79B9_7F4A_7C15) | 1)
    }

    pub fn next_u64(&mut self) -> u64 {
        let mut x = self.0;
        x ^= x >> 12;
        x ^= x << 25;
        x ^= x >> 27;
        self.0 = x;
        x.wrapping_mul(0x2545_F491_4F6C_DD1D)
    }

    pub fn below(&mut self, n: usize) -> usize {
        if n == 0 {
            0
        } else {
            (self.next_u64() % n as u64) as usize
        }
    }
}

/// Number of cases per fuzz target: `default`, or `FUZZ_ITERS` when set.
pub fn iterations(default: usize) -> usize {
    std::env::var("FUZZ_ITERS")
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(default)
}

const JUNK: &[&[u8]] = &[
    b"\0",
    b"\xff\xfe",
    b"{",
    b"}",
    b"[",
    b"]",
    b"(",
    b")",
    b"\"",
    b"\\",
    b",",
    b":",
    b";",
    b"*",
    b"%",
    b"-1",
    b"99999999999999999999",
    b"1e999",
    b"NaN",
    b"null",
    b"true",
    b"..",
    b"->",
    b"\n",
    b"\r\n",
    b"\t",
    b"G36*",
    b"M02*",
    b"%FSLAX46Y46*%",
    b"D03*",
    b"X-",
    b"\xe2\x82\xac",
    b"\xf0\x9f\x92\xa5",
];

/// One mutated copy of `input`.
pub fn mutate(input: &[u8], rng: &mut Rng) -> Vec<u8> {
    let mut v = input.to_vec();
    let rounds = 1 + rng.below(4);
    for _ in 0..rounds {
        if v.is_empty() {
            v.extend_from_slice(JUNK[rng.below(JUNK.len())]);
            continue;
        }
        let at = rng.below(v.len());
        match rng.below(9) {
            0 => v[at] ^= 1 << rng.below(8),
            1 => v[at] = rng.next_u64() as u8,
            2 => v.truncate(at),
            3 => {
                let end = (at + 1 + rng.below(64)).min(v.len());
                v.drain(at..end);
            }
            4 => {
                let end = (at + 1 + rng.below(64)).min(v.len());
                let chunk = v[at..end].to_vec();
                let to = rng.below(v.len());
                v.splice(to..to, chunk);
            }
            5 => {
                let j = JUNK[rng.below(JUNK.len())];
                v.splice(at..at, j.iter().copied());
            }
            6 => {
                // Swap two nearby bytes.
                let b = (at + 1 + rng.below(8)).min(v.len() - 1);
                v.swap(at, b);
            }
            7 => {
                // Replace a run of digits with a huge or negative number.
                if let Some(s) = v[at..].iter().position(u8::is_ascii_digit) {
                    let st = at + s;
                    let e = st + v[st..].iter().take_while(|c| c.is_ascii_digit()).count();
                    let repl: &[u8] = [
                        b"0".as_slice(),
                        b"-1",
                        b"4294967296",
                        b"9223372036854775807",
                        b"18446744073709551616",
                    ][rng.below(5)];
                    v.splice(st..e, repl.iter().copied());
                }
            }
            _ => {
                let n = rng.below(256);
                v.extend((0..n).map(|_| rng.next_u64() as u8));
            }
        }
    }
    v
}

/// `n` mutations of `input`, deterministic for a given `seed`.
pub fn mutations(input: &[u8], seed: u64, n: usize) -> impl Iterator<Item = Vec<u8>> + '_ {
    let mut rng = Rng::new(seed);
    (0..n).map(move |_| mutate(input, &mut rng))
}

/// Lossy text view for parsers that take `&str`.
pub fn as_text(bytes: &[u8]) -> String {
    String::from_utf8_lossy(bytes).into_owned()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn deterministic_and_actually_different() {
        let a: Vec<_> = mutations(b"hello world, this is a test 12345", 7, 50).collect();
        let b: Vec<_> = mutations(b"hello world, this is a test 12345", 7, 50).collect();
        assert_eq!(a, b);
        assert!(
            a.iter()
                .filter(|m| m.as_slice() != b"hello world, this is a test 12345")
                .count()
                > 40
        );
        assert!(mutate(b"", &mut Rng::new(1)).len() <= 1 + 256 + 8);
        assert_eq!(iterations(10), iterations(10));
    }
}
