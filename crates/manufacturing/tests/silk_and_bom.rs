//! Silkscreen text, BOM purchasing data and pick-and-place rotation offsets, from a real board
//! built through the Command Engine and written to real Gerber/CSV files, then read back.
#![allow(clippy::unwrap_used, clippy::panic, clippy::expect_used)]

use buturecad_eda_core::commands::{PcbEditCommand, SetFootprintCommand};
use buturecad_eda_core::model::{PcbChange, PcbObject};
use buturecad_eda_core::{CommandEngine, Project};
use buturecad_geometry::Point;
use buturecad_manufacturing::board::{build_fab_input_for_project, BuildOptions};
use buturecad_manufacturing::gerber_read::{self, Prim};
use buturecad_manufacturing::{generate, tables};
use buturecad_pcb::generators::chip;
use buturecad_pcb::{BoardGraphic, FpShape, PlacedFootprint, Placement};

const MM: i64 = 1_000_000;

fn project(rotation: u8, flipped: bool) -> Project {
    let mut e = CommandEngine::new(Project::new("silk").unwrap());
    let fp = chip("0603").unwrap();
    let name = fp.name.clone();
    e.execute(SetFootprintCommand::new(&name, None, Some(fp)))
        .unwrap();
    let mut ch = vec![PcbChange::create(
        e.new_id(),
        PcbObject::Footprint(PlacedFootprint {
            footprint: name,
            reference: "R17".into(),
            value: "10k".into(),
            placement: Placement {
                position: Point::new(20 * MM, 15 * MM),
                rotation,
                flipped,
            },
            locked: false,
            link: None,
            pad_nets: Default::default(),
        }),
    )];
    let w = [(0, 0), (40, 0), (40, 30), (0, 30)];
    for i in 0..4 {
        let (a, b) = (w[i], w[(i + 1) % 4]);
        ch.push(PcbChange::create(
            e.new_id(),
            PcbObject::Graphic(BoardGraphic {
                layer: buturecad_pcb::layer::EDGE_CUTS,
                shape: FpShape::Line {
                    segment: buturecad_geometry::Segment::new(
                        Point::new(a.0 * MM, a.1 * MM),
                        Point::new(b.0 * MM, b.1 * MM),
                    ),
                    width: 100_000,
                },
            }),
        ));
    }
    e.execute(PcbEditCommand::new("setup", ch)).unwrap();
    e.into_project()
}

/// Strokes of the silkscreen file that lie within `radius` of `c`: their bounding box.
/// Strokes inside a 3.2 x 1.4 mm window around `c` (the reference text of a chip footprint is
/// alone there): their bounding box. `rotated` swaps the window for a quarter-turned part.
fn strokes_near(gerber: &str, c: Point, rotated: bool) -> Option<(i64, i64, i64, i64, usize)> {
    let (rx, ry) = if rotated {
        (700_000, 1_600_000)
    } else {
        (1_600_000, 700_000)
    };
    let img = gerber_read::parse(gerber).unwrap();
    let (mut lo, mut hi, mut n) = ([i64::MAX; 2], [i64::MIN; 2], 0);
    for p in &img.prims {
        if let Prim::Stroke { a, b, .. } = p {
            for q in [a, b] {
                if (q[0] - c.x).abs() < rx && (q[1] - c.y).abs() < ry {
                    lo = [lo[0].min(q[0]), lo[1].min(q[1])];
                    hi = [hi[0].max(q[0]), hi[1].max(q[1])];
                }
            }
            if (a[0] - c.x).abs() < rx && (a[1] - c.y).abs() < ry {
                n += 1;
            }
        }
    }
    (n > 0).then_some((lo[0], lo[1], hi[0], hi[1], n))
}

fn silk(p: &Project, flipped: bool) -> String {
    let g = generate(&build_fab_input_for_project(
        p,
        "silk",
        &BuildOptions::default(),
    ))
    .unwrap();
    let want = if flipped {
        "silk-B_Silkscreen.gbo"
    } else {
        "silk-F_Silkscreen.gto"
    };
    String::from_utf8(
        g.files
            .iter()
            .find(|f| f.name == want)
            .unwrap_or_else(|| panic!("no {want}"))
            .bytes
            .clone(),
    )
    .unwrap()
}

fn text_centre(p: &Project) -> Point {
    let doc = p.pcb();
    let f = doc.placed.values().next().unwrap();
    let def = &doc.footprints[&f.footprint];
    let t = def
        .texts
        .iter()
        .find(|t| t.kind == buturecad_pcb::TextKind::Reference)
        .unwrap();
    f.placement.apply(t.position)
}

#[test]
fn the_reference_is_written_on_the_silkscreen_where_the_footprint_says() {
    let p = project(0, false);
    let c = text_centre(&p);
    let (x0, y0, x1, y1, n) =
        strokes_near(&silk(&p, false), c, false).expect("the reference text has strokes");
    assert!(n >= 10, "'R17' is several strokes, found {n}");
    // 'R17' at 1.0 mm cap height is (3*6-2)/6 = 2.667 mm wide and 1.0 mm tall, centred on the text position.
    let (w, h) = (x1 - x0, y1 - y0);
    assert!(
        (w - 2_666_666).abs() < 20_000 && (h - 1_000_000).abs() < 20_000,
        "{w} x {h}"
    );
    assert!(((x0 + x1) / 2 - c.x).abs() < 20_000 && ((y0 + y1) / 2 - c.y).abs() < 20_000);
}

#[test]
fn text_turns_with_the_part_and_is_mirrored_on_the_bottom() {
    // Rotated 90°: the text block is tall instead of wide.
    let p = project(1, false);
    let c = text_centre(&p);
    let (x0, y0, x1, y1, _) = strokes_near(&silk(&p, false), c, true).unwrap();
    assert!(
        (y1 - y0 - 2_666_666).abs() < 20_000 && (x1 - x0 - 1_000_000).abs() < 20_000,
        "{} x {}",
        x1 - x0,
        y1 - y0
    );
    // Flipped: it moves to the bottom silkscreen (and is not on the top).
    let p = project(0, true);
    let c = text_centre(&p);
    assert!(
        strokes_near(&silk(&p, true), c, false).is_some(),
        "text is on the bottom silkscreen"
    );
    let g = generate(&build_fab_input_for_project(
        &p,
        "silk",
        &BuildOptions::default(),
    ))
    .unwrap();
    assert!(
        g.files.iter().all(|f| f.name != "silk-F_Silkscreen.gto")
            || strokes_near(&silk(&p, false), c, false).is_none()
    );
    // Mirrored: the 'R' of "R17" (first character, reading left to right from the bottom) ends on the
    // right side in board coordinates.
    let (x0, _, x1, _, _) = strokes_near(&silk(&p, true), c, false).unwrap();
    assert!(((x0 + x1) / 2 - c.x).abs() < 20_000, "still centred");
}

#[test]
fn values_are_printed_only_when_asked() {
    let p = project(0, false);
    let plain = silk(&p, false);
    let with = {
        let g = generate(&build_fab_input_for_project(
            &p,
            "silk",
            &BuildOptions {
                silk_values: true,
                ..Default::default()
            },
        ))
        .unwrap();
        String::from_utf8(
            g.files
                .iter()
                .find(|f| f.name == "silk-F_Silkscreen.gto")
                .unwrap()
                .bytes
                .clone(),
        )
        .unwrap()
    };
    assert!(with.len() > plain.len(), "value strokes were added");
}

#[test]
fn bom_groups_by_purchasing_data_and_position_file_applies_offsets() {
    use buturecad_manufacturing::{Mount, Part, PartFields, Pt};
    let part = |r: &str, mpn: &str, fp: &str, rot: u16| Part {
        reference: r.into(),
        value: "10k".into(),
        footprint: fp.into(),
        position: Pt { x: MM, y: 2 * MM },
        rotation_deg: rot,
        bottom: false,
        mount: Mount::Smd,
        in_bom: true,
        in_position_file: true,
        fields: PartFields {
            manufacturer: "Yageo".into(),
            mpn: mpn.into(),
            supplier: String::new(),
            supplier_pn: String::new(),
        },
    };
    let parts = vec![
        part("R1", "RC0603FR-0710KL", "Chip_0603", 0),
        part("R2", "RC0603FR-0710KL", "Chip_0603", 0),
        part("R3", "ERJ-3EKF1002V", "Chip_0603", 0),
        part("U1", "X", "SOT-23-5", 90),
    ];
    let bom = tables::bom_csv(&parts);
    let lines: Vec<&str> = bom.lines().collect();
    assert_eq!(
        lines[0],
        "Reference,Quantity,Value,Footprint,Manufacturer,MPN,Supplier,SPN"
    );
    assert!(
        lines.contains(&"R1 R2,2,10k,Chip_0603,Yageo,RC0603FR-0710KL,,"),
        "{bom}"
    );
    assert!(
        lines.contains(&"R3,1,10k,Chip_0603,Yageo,ERJ-3EKF1002V,,"),
        "same value, other MPN: its own line"
    );

    let offsets = tables::parse_rotation_offsets(
        "# JLC-style corrections\nSOT-23*, 180\n\"Chip_*\",90\nSOT-23-5, 45\n",
    )
    .unwrap();
    assert_eq!(offsets.len(), 3);
    let pos = tables::pick_and_place_csv(&parts, &offsets);
    assert!(
        pos.contains("U1,10k,SOT-23-5,1.000000,2.000000,270,top"),
        "90 + 180 (first match wins) = 270\n{pos}"
    );
    assert!(
        pos.contains("R1,10k,Chip_0603,1.000000,2.000000,90,top"),
        "{pos}"
    );
    assert!(tables::pick_and_place_csv(&parts, &[]).contains(",0,top"));
    // Wrap-around and negative offsets.
    assert_eq!(
        tables::placed_rotation(&parts[3], &[("*".into(), -135)]),
        315
    );
}

#[test]
fn rotation_offset_files_are_validated() {
    assert!(tables::parse_rotation_offsets("SOT-23*").is_err());
    assert!(tables::parse_rotation_offsets("SOT-23*,abc").is_err());
    assert!(tables::parse_rotation_offsets(",90").is_err());
    assert_eq!(
        tables::parse_rotation_offsets("\n  # only a comment\n").unwrap(),
        vec![]
    );
    assert!(tables::glob_match("sot-23*", "SOT-23-5"));
    assert!(tables::glob_match("*0603", "Chip_0603"));
    assert!(tables::glob_match("a*b*c", "aXXbYYc") && !tables::glob_match("a*b*c", "aXXbYY"));
}

/// Endpoints of every stroke in the 3.2 x 1.4 mm window, as a sorted list of normalised segments.
fn segments(gerber: &str, c: Point) -> Vec<([i64; 2], [i64; 2])> {
    let img = gerber_read::parse(gerber).unwrap();
    let mut v: Vec<_> = img
        .prims
        .iter()
        .filter_map(|p| match p {
            Prim::Stroke { a, b, .. }
                if (a[0] - c.x).abs() < 1_600_000
                    && (a[1] - c.y).abs() < 700_000
                    && (b[0] - c.x).abs() < 1_600_000
                    && (b[1] - c.y).abs() < 700_000 =>
            {
                Some(if a <= b { (*a, *b) } else { (*b, *a) })
            }
            _ => None,
        })
        .collect();
    v.sort();
    v
}

#[test]
fn bottom_text_is_the_exact_mirror_of_top_text() {
    let (top, bottom) = (project(0, false), project(0, true));
    let c = text_centre(&top);
    assert_eq!(
        c,
        text_centre(&bottom),
        "the text is on the part's axis, so mirroring keeps its centre"
    );
    let t = segments(&silk(&top, false), c);
    let mut b: Vec<_> = segments(&silk(&bottom, true), c);
    // Mirror the top text about x = c.x and compare.
    let mut m: Vec<_> = t
        .iter()
        .map(|(a, b)| {
            let (a, b) = ([2 * c.x - a[0], a[1]], [2 * c.x - b[0], b[1]]);
            if a <= b {
                (a, b)
            } else {
                (b, a)
            }
        })
        .collect();
    m.sort();
    b.sort();
    assert!(t.len() >= 10);
    assert_eq!(
        m, b,
        "bottom silkscreen text reads correctly when the board is turned over"
    );
}
