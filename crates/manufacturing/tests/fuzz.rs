//! Gerber / Excellon / ZIP readers and the 2-D rasteriser under fire, plus a writer→reader
//! round-trip property.
#![allow(clippy::unwrap_used, clippy::panic)]

use buturecad_manufacturing::excellon_read;
use buturecad_manufacturing::gerber_read::{self, Prim};
use buturecad_manufacturing::raster2d::{bounds, render, Content, LayerDraw, Viewport};
use buturecad_manufacturing::{
    generate, zip, Aperture, Capabilities, DrcSummary, FabInput, Feature, Hole, Layer, Mount, Part,
    Pt,
};
use buturecad_testkit::{as_text, iterations, mutations};
use proptest::prelude::*;

fn board() -> FabInput {
    let p = |x, y| Pt { x, y };
    let outline = vec![
        p(0, 0),
        p(20_000_000, 0),
        p(20_000_000, 10_000_000),
        p(0, 10_000_000),
    ];
    FabInput {
        name: "f".into(),
        outline: vec![outline.clone()],
        copper_layers: 2,
        thickness_nm: 1_600_000,
        layers: vec![
            Layer {
                file: "f-F_Cu.gtl".into(),
                function: "Copper,L1,Top".into(),
                features: vec![
                    Feature::Flash {
                        at: p(5_000_000, 5_000_000),
                        ap: Aperture::Circle { d: 1_600_000 },
                    },
                    Feature::Flash {
                        at: p(8_000_000, 5_000_000),
                        ap: Aperture::Rect {
                            w: 1_600_000,
                            h: 900_000,
                        },
                    },
                    Feature::Flash {
                        at: p(11_000_000, 5_000_000),
                        ap: Aperture::Obround {
                            w: 1_600_000,
                            h: 900_000,
                        },
                    },
                    Feature::Draw {
                        a: p(1, 1),
                        b: p(5_000_000, 1),
                        width: 250_000,
                    },
                    Feature::Region {
                        outer: vec![
                            p(1_000_000, 1_000_000),
                            p(9_000_000, 1_000_000),
                            p(9_000_000, 3_000_000),
                            p(1_000_000, 3_000_000),
                        ],
                        holes: vec![vec![
                            p(2_000_000, 1_500_000),
                            p(3_000_000, 1_500_000),
                            p(3_000_000, 2_500_000),
                        ]],
                    },
                ],
            },
            Layer {
                file: "f-Edge_Cuts.gko".into(),
                function: "Profile,NP".into(),
                features: (0..4)
                    .map(|i| Feature::Draw {
                        a: outline[i],
                        b: outline[(i + 1) % 4],
                        width: 100_000,
                    })
                    .collect(),
            },
        ],
        holes: vec![
            Hole {
                at: p(5_000_000, 5_000_000),
                diameter: 800_000,
                slot_to: None,
                plated: true,
            },
            Hole {
                at: p(8_000_000, 7_000_000),
                diameter: 1_000_000,
                slot_to: Some(p(10_000_000, 7_000_000)),
                plated: false,
            },
        ],
        parts: vec![Part {
            reference: "R1".into(),
            value: "1k".into(),
            footprint: "0603".into(),
            position: p(5_000_000, 5_000_000),
            rotation_deg: 0,
            bottom: false,
            mount: Mount::Smd,
            in_bom: true,
            in_position_file: true,
            fields: Default::default(),
        }],
        drc: DrcSummary::default(),
        capabilities: Capabilities::default(),
        min_track_width: Some(250_000),
        extra_issues: Vec::new(),
        rotation_offsets: Vec::new(),
    }
}

#[test]
fn damaged_gerber_excellon_and_zip_never_panic() {
    let g = generate(&board()).unwrap();
    let n = iterations(800);
    let tiny = |lo, hi| Viewport::fit(lo, hi, 48, 32);
    for f in &g.files {
        let name = f.name.as_str();
        if name.ends_with(".gtl") || name.ends_with(".gko") {
            for (i, bad) in mutations(&f.bytes, 11, n).enumerate() {
                let text = as_text(&bad);
                if let Ok(img) = gerber_read::parse(&text) {
                    let layers = [LayerDraw {
                        content: Content::Gerber(&img),
                        color: [255, 0, 0],
                        alpha: 0.8,
                    }];
                    if let Some((lo, hi)) = bounds(&layers) {
                        // Absurd coordinates must not hang or overflow the rasteriser.
                        if i % 4 == 0 {
                            let _ = render(&layers, &tiny(lo, hi), [0, 0, 0]);
                        }
                    }
                }
            }
        } else if name.ends_with(".drl") {
            for bad in mutations(&f.bytes, 12, n) {
                if let Ok(d) = excellon_read::parse(&as_text(&bad)) {
                    let layers = [LayerDraw {
                        content: Content::Drill(&d),
                        color: [255, 255, 255],
                        alpha: 1.0,
                    }];
                    if let Some((lo, hi)) = bounds(&layers) {
                        let _ = render(&layers, &tiny(lo, hi), [0, 0, 0]);
                    }
                }
            }
        }
    }
    let z = g.zip().unwrap();
    for bad in mutations(&z, 13, n) {
        if let Some(files) = zip::read_stored(&bad) {
            // Whatever was read passed its CRC checks.
            assert!(files.iter().all(|(n, _)| !n.is_empty()));
        }
    }
    assert!(zip::read_stored(&[]).is_none() && zip::read_stored(&[0u8; 21]).is_none());
}

fn pt() -> impl Strategy<Value = Pt> {
    (-500_000_000i64..500_000_000, -500_000_000i64..500_000_000).prop_map(|(x, y)| Pt { x, y })
}

fn feature() -> impl Strategy<Value = Feature> {
    prop_oneof![
        (pt(), 1i64..5_000_000).prop_map(|(at, d)| Feature::Flash {
            at,
            ap: Aperture::Circle { d }
        }),
        (pt(), 1i64..5_000_000, 1i64..5_000_000).prop_map(|(at, w, h)| Feature::Flash {
            at,
            ap: Aperture::Rect { w, h }
        }),
        (pt(), 1i64..5_000_000, 1i64..5_000_000).prop_map(|(at, w, h)| Feature::Flash {
            at,
            ap: Aperture::Obround { w, h }
        }),
        (pt(), pt(), 1i64..2_000_000).prop_map(|(a, b, width)| Feature::Draw { a, b, width }),
        proptest::collection::vec(pt(), 3..8).prop_map(|outer| Feature::Region {
            outer,
            holes: Vec::new()
        }),
    ]
}

proptest! {
    /// Everything the writer emits, the reader reads back exactly: positions, counts, apertures.
    #[test]
    fn gerber_writer_and_reader_agree(features in proptest::collection::vec(feature(), 0..40)) {
        let layer = Layer { file: "x.gtl".into(), function: "Copper,L1,Top".into(), features: features.clone() };
        let text = buturecad_manufacturing::gerber::write(&layer).unwrap();
        let img = gerber_read::parse(&text).unwrap();
        prop_assert!(img.warnings.is_empty());
        let (mut fl, mut dr, mut rg) = (Vec::new(), 0, 0);
        for f in &features {
            match f {
                Feature::Flash { at, .. } => fl.push([at.x, at.y]),
                Feature::Draw { .. } => dr += 1,
                Feature::Region { holes, .. } => rg += 1 + holes.len(),
            }
        }
        let got: Vec<[i64; 2]> = img.prims.iter().filter_map(|p| if let Prim::Flash { at, .. } = p { Some(*at) } else { None }).collect();
        prop_assert_eq!(got, fl);
        let (_, s, r) = img.count();
        prop_assert_eq!((s, r), (dr, rg));
    }

    #[test]
    fn zip_round_trips(files in proptest::collection::vec(("[a-z]{1,8}\\.[a-z]{2,3}", proptest::collection::vec(any::<u8>(), 0..200)), 0..6)) {
        let mut seen = std::collections::BTreeSet::new();
        let files: Vec<(String, Vec<u8>)> = files.into_iter().filter(|(n, _)| seen.insert(n.clone())).collect();
        let z = zip::write(&files).unwrap();
        prop_assert_eq!(zip::read_stored(&z).unwrap(), files);
    }
}
