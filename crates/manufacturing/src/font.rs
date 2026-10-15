//! A single-stroke font for silkscreen text.
//!
//! Silkscreen is drawn with lines of a fixed width, so text must be strokes, not filled
//! outlines or a bitmap. The glyphs are ButureCAD's own, designed on a 4 × 6 grid (capital
//! height 6 units, lower-case x-height 4, descenders to −2); nothing is copied from another
//! font. Covered: `0-9`, `A-Z`, `a-z` and `- + . , _ / : ( ) = * % ° µ Ω space`. Anything else is
//! drawn as an open box so that a missing character is visible on the board instead of silently
//! dropped.
//!
//! Layout: the advance is 6 units per character (4 wide + 2 gap); a text of height `h` nm uses
//! `h / 6` nm per unit. Coordinates returned are relative to the **centre** of the text block.

use crate::Pt;

/// Polylines of one glyph on the 4 × 6 grid.
type Glyph = &'static [&'static [(i8, i8)]];

const BOX: Glyph = &[&[(0, 0), (4, 0), (4, 6), (0, 6), (0, 0)]];

fn glyph(c: char) -> Option<Glyph> {
    Some(match c {
        ' ' => &[],
        '0' => &[
            &[
                (1, 0),
                (0, 1),
                (0, 5),
                (1, 6),
                (3, 6),
                (4, 5),
                (4, 1),
                (3, 0),
                (1, 0),
            ],
            &[(0, 1), (4, 5)],
        ],
        '1' => &[&[(1, 5), (2, 6), (2, 0)], &[(0, 0), (4, 0)]],
        '2' => &[&[(0, 5), (1, 6), (3, 6), (4, 5), (4, 4), (0, 0), (4, 0)]],
        '3' => &[&[
            (0, 6),
            (4, 6),
            (2, 4),
            (3, 4),
            (4, 3),
            (4, 1),
            (3, 0),
            (1, 0),
            (0, 1),
        ]],
        '4' => &[&[(3, 0), (3, 6), (0, 2), (4, 2)]],
        '5' => &[&[
            (4, 6),
            (0, 6),
            (0, 3),
            (3, 3),
            (4, 2),
            (4, 1),
            (3, 0),
            (0, 0),
        ]],
        '6' => &[&[
            (4, 6),
            (1, 6),
            (0, 5),
            (0, 1),
            (1, 0),
            (3, 0),
            (4, 1),
            (4, 2),
            (3, 3),
            (0, 3),
        ]],
        '7' => &[&[(0, 6), (4, 6), (1, 0)]],
        '8' => &[&[
            (1, 3),
            (0, 4),
            (0, 5),
            (1, 6),
            (3, 6),
            (4, 5),
            (4, 4),
            (3, 3),
            (1, 3),
            (0, 2),
            (0, 1),
            (1, 0),
            (3, 0),
            (4, 1),
            (4, 2),
            (3, 3),
        ]],
        '9' => &[&[
            (0, 0),
            (3, 0),
            (4, 1),
            (4, 5),
            (3, 6),
            (1, 6),
            (0, 5),
            (0, 4),
            (1, 3),
            (4, 3),
        ]],
        'A' => &[&[(0, 0), (0, 4), (2, 6), (4, 4), (4, 0)], &[(0, 3), (4, 3)]],
        'B' => &[
            &[(0, 0), (0, 6), (3, 6), (4, 5), (4, 4), (3, 3), (0, 3)],
            &[(3, 3), (4, 2), (4, 1), (3, 0), (0, 0)],
        ],
        'C' => &[&[
            (4, 5),
            (3, 6),
            (1, 6),
            (0, 5),
            (0, 1),
            (1, 0),
            (3, 0),
            (4, 1),
        ]],
        'D' => &[&[(0, 0), (0, 6), (2, 6), (4, 4), (4, 2), (2, 0), (0, 0)]],
        'E' => &[&[(4, 6), (0, 6), (0, 0), (4, 0)], &[(0, 3), (3, 3)]],
        'F' => &[&[(4, 6), (0, 6), (0, 0)], &[(0, 3), (3, 3)]],
        'G' => &[&[
            (4, 5),
            (3, 6),
            (1, 6),
            (0, 5),
            (0, 1),
            (1, 0),
            (3, 0),
            (4, 1),
            (4, 3),
            (2, 3),
        ]],
        'H' => &[&[(0, 0), (0, 6)], &[(4, 0), (4, 6)], &[(0, 3), (4, 3)]],
        'I' => &[&[(1, 6), (3, 6)], &[(2, 6), (2, 0)], &[(1, 0), (3, 0)]],
        'J' => &[&[(1, 6), (4, 6)], &[(3, 6), (3, 1), (2, 0), (1, 0), (0, 1)]],
        'K' => &[&[(0, 0), (0, 6)], &[(4, 6), (0, 3), (4, 0)]],
        'L' => &[&[(0, 6), (0, 0), (4, 0)]],
        'M' => &[&[(0, 0), (0, 6), (2, 3), (4, 6), (4, 0)]],
        'N' => &[&[(0, 0), (0, 6), (4, 0), (4, 6)]],
        'O' => &[&[
            (1, 0),
            (0, 1),
            (0, 5),
            (1, 6),
            (3, 6),
            (4, 5),
            (4, 1),
            (3, 0),
            (1, 0),
        ]],
        'P' => &[&[(0, 0), (0, 6), (3, 6), (4, 5), (4, 4), (3, 3), (0, 3)]],
        'Q' => &[
            &[
                (1, 0),
                (0, 1),
                (0, 5),
                (1, 6),
                (3, 6),
                (4, 5),
                (4, 1),
                (3, 0),
                (1, 0),
            ],
            &[(2, 2), (4, 0)],
        ],
        'R' => &[
            &[(0, 0), (0, 6), (3, 6), (4, 5), (4, 4), (3, 3), (0, 3)],
            &[(2, 3), (4, 0)],
        ],
        'S' => &[&[
            (4, 5),
            (3, 6),
            (1, 6),
            (0, 5),
            (0, 4),
            (1, 3),
            (3, 3),
            (4, 2),
            (4, 1),
            (3, 0),
            (1, 0),
            (0, 1),
        ]],
        'T' => &[&[(0, 6), (4, 6)], &[(2, 6), (2, 0)]],
        'U' => &[&[(0, 6), (0, 1), (1, 0), (3, 0), (4, 1), (4, 6)]],
        'V' => &[&[(0, 6), (2, 0), (4, 6)]],
        'W' => &[&[(0, 6), (1, 0), (2, 3), (3, 0), (4, 6)]],
        'X' => &[&[(0, 6), (4, 0)], &[(4, 6), (0, 0)]],
        'Y' => &[&[(0, 6), (2, 3), (4, 6)], &[(2, 3), (2, 0)]],
        'Z' => &[&[(0, 6), (4, 6), (0, 0), (4, 0)]],
        'a' => &[
            &[
                (4, 3),
                (3, 4),
                (1, 4),
                (0, 3),
                (0, 1),
                (1, 0),
                (3, 0),
                (4, 1),
            ],
            &[(4, 4), (4, 0)],
        ],
        'b' => &[
            &[(0, 6), (0, 0)],
            &[
                (0, 3),
                (1, 4),
                (3, 4),
                (4, 3),
                (4, 1),
                (3, 0),
                (1, 0),
                (0, 1),
            ],
        ],
        'c' => &[&[
            (4, 3),
            (3, 4),
            (1, 4),
            (0, 3),
            (0, 1),
            (1, 0),
            (3, 0),
            (4, 1),
        ]],
        'd' => &[
            &[(4, 6), (4, 0)],
            &[
                (4, 3),
                (3, 4),
                (1, 4),
                (0, 3),
                (0, 1),
                (1, 0),
                (3, 0),
                (4, 1),
            ],
        ],
        'e' => &[&[
            (0, 2),
            (4, 2),
            (4, 3),
            (3, 4),
            (1, 4),
            (0, 3),
            (0, 1),
            (1, 0),
            (3, 0),
            (4, 1),
        ]],
        'f' => &[&[(4, 6), (3, 6), (2, 5), (2, 0)], &[(1, 4), (4, 4)]],
        'g' => &[
            &[
                (4, 3),
                (3, 4),
                (1, 4),
                (0, 3),
                (0, 1),
                (1, 0),
                (3, 0),
                (4, 1),
            ],
            &[(4, 4), (4, -1), (3, -2), (1, -2), (0, -1)],
        ],
        'h' => &[&[(0, 6), (0, 0)], &[(0, 3), (1, 4), (3, 4), (4, 3), (4, 0)]],
        'i' => &[&[(2, 4), (2, 0)], &[(2, 5), (2, 6)]],
        'j' => &[&[(2, 4), (2, -1), (1, -2), (0, -2)], &[(2, 5), (2, 6)]],
        'k' => &[&[(0, 6), (0, 0)], &[(4, 4), (0, 1)], &[(1, 2), (4, 0)]],
        'l' => &[&[(1, 6), (2, 6), (2, 1), (3, 0)]],
        'm' => &[
            &[(0, 0), (0, 4)],
            &[(0, 3), (1, 4), (2, 3), (2, 0)],
            &[(2, 3), (3, 4), (4, 3), (4, 0)],
        ],
        'n' => &[&[(0, 0), (0, 4)], &[(0, 3), (1, 4), (3, 4), (4, 3), (4, 0)]],
        'o' => &[&[
            (1, 0),
            (0, 1),
            (0, 3),
            (1, 4),
            (3, 4),
            (4, 3),
            (4, 1),
            (3, 0),
            (1, 0),
        ]],
        'p' => &[
            &[(0, -2), (0, 4)],
            &[
                (0, 3),
                (1, 4),
                (3, 4),
                (4, 3),
                (4, 1),
                (3, 0),
                (1, 0),
                (0, 1),
            ],
        ],
        'q' => &[
            &[(4, -2), (4, 4)],
            &[
                (4, 3),
                (3, 4),
                (1, 4),
                (0, 3),
                (0, 1),
                (1, 0),
                (3, 0),
                (4, 1),
            ],
        ],
        'r' => &[&[(0, 0), (0, 4)], &[(0, 3), (1, 4), (3, 4), (4, 3)]],
        's' => &[&[
            (4, 3),
            (3, 4),
            (1, 4),
            (0, 3),
            (1, 2),
            (3, 2),
            (4, 1),
            (3, 0),
            (1, 0),
            (0, 1),
        ]],
        't' => &[&[(2, 6), (2, 1), (3, 0), (4, 0)], &[(0, 4), (4, 4)]],
        'u' => &[&[(0, 4), (0, 1), (1, 0), (3, 0), (4, 1)], &[(4, 4), (4, 0)]],
        'v' => &[&[(0, 4), (2, 0), (4, 4)]],
        'w' => &[&[(0, 4), (1, 0), (2, 3), (3, 0), (4, 4)]],
        'x' => &[&[(0, 4), (4, 0)], &[(4, 4), (0, 0)]],
        'y' => &[&[(0, 4), (2, 0)], &[(4, 4), (1, -2), (0, -2)]],
        'z' => &[&[(0, 4), (4, 4), (0, 0), (4, 0)]],
        '-' => &[&[(1, 3), (3, 3)]],
        '+' => &[&[(1, 3), (3, 3)], &[(2, 2), (2, 4)]],
        '.' => &[&[(2, 0), (2, 1)]],
        ',' => &[&[(2, 1), (2, 0), (1, -1)]],
        '_' => &[&[(0, 0), (4, 0)]],
        '/' => &[&[(0, 0), (4, 6)]],
        ':' => &[&[(2, 1), (2, 2)], &[(2, 4), (2, 5)]],
        '(' => &[&[(3, 6), (2, 5), (2, 1), (3, 0)]],
        ')' => &[&[(1, 6), (2, 5), (2, 1), (1, 0)]],
        '=' => &[&[(0, 2), (4, 2)], &[(0, 4), (4, 4)]],
        '*' => &[&[(2, 1), (2, 5)], &[(0, 2), (4, 4)], &[(0, 4), (4, 2)]],
        '%' => &[
            &[(0, 0), (4, 6)],
            &[(0, 4), (1, 4), (1, 6), (0, 6), (0, 4)],
            &[(3, 0), (4, 0), (4, 2), (3, 2), (3, 0)],
        ],
        '°' => &[&[(1, 5), (1, 6), (2, 6), (2, 5), (1, 5)]],
        'µ' => &[
            &[(0, 4), (0, -2)],
            &[(0, 1), (1, 0), (3, 0), (4, 1)],
            &[(4, 4), (4, 0)],
        ],
        'Ω' => &[&[
            (0, 0),
            (1, 0),
            (0, 2),
            (0, 4),
            (1, 6),
            (3, 6),
            (4, 4),
            (4, 2),
            (3, 0),
            (4, 0),
        ]],
        _ => return None,
    })
}

/// Units per character advance and per glyph width / height (grid units).
pub const ADVANCE: i64 = 6;
pub const CAP_HEIGHT: i64 = 6;

/// Stroked text: polylines in nm around the centre of the text block, and the block's size.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Strokes {
    pub polylines: Vec<Vec<Pt>>,
    pub width: i64,
    pub height: i64,
    /// Characters that have no glyph and were drawn as a box.
    pub unknown: usize,
}

/// Width of `text` in nm for a text of cap height `height`.
pub fn text_width(text: &str, height: i64) -> i64 {
    let n = text.chars().count() as i64;
    if n == 0 {
        return 0;
    }
    (n * ADVANCE - 2) * height / CAP_HEIGHT
}

/// Strokes of `text`, `height` nm tall (capital letters), centred on (0, 0).
pub fn strokes(text: &str, height: i64) -> Strokes {
    let n = text.chars().count() as i64;
    let unit = |v: i64| v * height / CAP_HEIGHT;
    let width = text_width(text, height);
    let (x0, y0) = (-width / 2, -height / 2);
    let mut polylines = Vec::new();
    let mut unknown = 0;
    for (i, c) in text.chars().enumerate() {
        let g = match glyph(c) {
            Some(g) => g,
            None => {
                unknown += 1;
                BOX
            }
        };
        let ox = x0 + unit(i as i64 * ADVANCE);
        for line in g {
            polylines.push(
                line.iter()
                    .map(|&(x, y)| Pt {
                        x: ox + unit(i64::from(x)),
                        y: y0 + unit(i64::from(y)),
                    })
                    .collect(),
            );
        }
    }
    let _ = n;
    Strokes {
        polylines,
        width,
        height,
        unknown,
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;

    #[test]
    fn every_printable_ascii_character_and_the_unit_symbols_have_a_glyph() {
        for c in (0x20u8..=0x7e).map(char::from) {
            let drawn = c.is_ascii_alphanumeric() || " -+.,_/:()=*%".contains(c);
            assert_eq!(glyph(c).is_some(), drawn, "{c:?}");
        }
        for c in ['°', 'µ', 'Ω'] {
            assert!(glyph(c).is_some(), "{c}");
        }
    }

    #[test]
    fn glyph_strokes_stay_on_the_grid() {
        for c in (0x20u8..=0x7e).map(char::from).chain(['°', 'µ', 'Ω']) {
            for line in glyph(c).unwrap_or(&[]) {
                assert!(line.len() >= 2, "{c:?} has a degenerate stroke");
                for &(x, y) in *line {
                    assert!(
                        (0..=4).contains(&x) && (-2..=6).contains(&y),
                        "{c:?}: ({x},{y})"
                    );
                }
            }
        }
    }

    #[test]
    fn layout_is_centred_and_scales_with_height() {
        let s = strokes("R10", 1_200_000);
        assert_eq!(s.width, (3 * 6 - 2) * 200_000);
        let (min_x, max_x) = s
            .polylines
            .iter()
            .flatten()
            .fold((i64::MAX, i64::MIN), |a, p| (a.0.min(p.x), a.1.max(p.x)));
        assert_eq!(min_x, -s.width / 2);
        assert!(max_x <= s.width / 2 + 1);
        let (min_y, max_y) = s
            .polylines
            .iter()
            .flatten()
            .fold((i64::MAX, i64::MIN), |a, p| (a.0.min(p.y), a.1.max(p.y)));
        assert_eq!((min_y, max_y), (-600_000, 600_000));
        // Double the height, double the size.
        assert_eq!(strokes("R10", 2_400_000).width, 2 * s.width);
        assert_eq!(text_width("", 1_000_000), 0);
        assert!(strokes("", 1_000_000).polylines.is_empty());
    }

    #[test]
    fn unknown_characters_are_visible_not_dropped() {
        let s = strokes("A#", 1_000_000);
        assert_eq!(s.unknown, 1);
        assert!(s.polylines.len() >= 3, "A (2 strokes) plus a box");
    }

    #[test]
    fn upper_and_lower_case_differ_where_units_need_it() {
        // 1M and 1m must not look the same on a board.
        assert_ne!(strokes("M", 1_000_000), strokes("m", 1_000_000));
        assert_ne!(strokes("K", 1_000_000), strokes("k", 1_000_000));
    }
}
