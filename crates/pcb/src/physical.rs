//! The physical layer stack of a board: copper and dielectric thicknesses and materials.
//!
//! [`Stackup`](crate::layer::Stackup) says *which* copper layers exist; a [`PhysicalStack`]
//! says what the board is made of between them. Dielectric constants and loss tangents are
//! stored as integers (milli-units and parts per million) so the model stays exactly comparable
//! and serialisable. Thicknesses are nanometres like every other length.
//!
//! The model is general enough for the stackups ButureCAD will grow into: any even number of
//! copper layers, several dielectric layers per gap (core + prepreg + prepreg…), thin build-up
//! dielectrics for HDI (micro-via spans) and flexible dielectrics (`flex`) for rigid-flex boards.
//! The 3D viewer, impedance calculation and manufacturing documentation will read it.

use crate::board::BoardError;
use crate::layer::{self, LayerId, Stackup};
use buturecad_geometry::Coord;
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

fn bad<T>(m: impl Into<String>) -> Result<T, BoardError> {
    Err(BoardError::Invalid(m.into()))
}

/// What a dielectric layer is for.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DielectricKind {
    /// Cured laminate with copper on both sides.
    Core,
    /// Uncured glass/resin sheet that bonds layers.
    Prepreg,
    /// Soldermask over an outer copper layer (outside the copper stack).
    SolderMask,
    /// Flexible cover film (rigid-flex).
    Coverlay,
    /// Adhesive between flex and rigid parts.
    Adhesive,
}

impl DielectricKind {
    pub const ALL: [DielectricKind; 5] = [
        DielectricKind::Core,
        DielectricKind::Prepreg,
        DielectricKind::SolderMask,
        DielectricKind::Coverlay,
        DielectricKind::Adhesive,
    ];
    pub fn id(self) -> &'static str {
        match self {
            DielectricKind::Core => "core",
            DielectricKind::Prepreg => "prepreg",
            DielectricKind::SolderMask => "solder_mask",
            DielectricKind::Coverlay => "coverlay",
            DielectricKind::Adhesive => "adhesive",
        }
    }
    pub fn parse(s: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|k| k.id() == s)
    }
    pub fn title(self) -> &'static str {
        match self {
            DielectricKind::Core => "Core",
            DielectricKind::Prepreg => "Prepreg",
            DielectricKind::SolderMask => "Solder mask",
            DielectricKind::Coverlay => "Coverlay",
            DielectricKind::Adhesive => "Adhesive",
        }
    }
}

/// What a copper layer is used for (informational for now; the router and the zone tool will
/// use it as a hint).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CopperRole {
    #[default]
    Signal,
    Plane,
    Mixed,
}

impl CopperRole {
    pub fn id(self) -> &'static str {
        match self {
            CopperRole::Signal => "signal",
            CopperRole::Plane => "plane",
            CopperRole::Mixed => "mixed",
        }
    }
    pub fn parse(s: &str) -> Option<Self> {
        match s {
            "signal" => Some(CopperRole::Signal),
            "plane" => Some(CopperRole::Plane),
            "mixed" => Some(CopperRole::Mixed),
            _ => None,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct CopperSpec {
    pub thickness: Coord,
    #[serde(default)]
    pub role: CopperRole,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Dielectric {
    pub kind: DielectricKind,
    pub material: String,
    pub thickness: Coord,
    /// Relative permittivity × 1000 (FR-4 ≈ 4 300).
    pub er_milli: u32,
    /// Loss tangent in parts per million (FR-4 ≈ 20 000).
    pub loss_tangent_ppm: u32,
    /// Flexible material (polyimide): rigid-flex boards mix flexible and rigid dielectrics.
    #[serde(default)]
    pub flex: bool,
}

impl Dielectric {
    pub fn new(kind: DielectricKind, material: &str, thickness: Coord, er_milli: u32) -> Self {
        Self {
            kind,
            material: material.into(),
            thickness,
            er_milli,
            loss_tangent_ppm: 20_000,
            flex: false,
        }
    }

    fn validate(&self, what: &str) -> Result<(), BoardError> {
        if self.material.trim().is_empty()
            || self.material.len() > 64
            || self.material.chars().any(char::is_control)
        {
            return bad(format!(
                "{what}: the material needs a name (up to 64 characters)"
            ));
        }
        if !(1_000..=10_000_000).contains(&self.thickness) {
            return bad(format!("{what}: thickness must be between 1 µm and 10 mm"));
        }
        if !(1_000..=100_000).contains(&self.er_milli) {
            return bad(format!(
                "{what}: relative permittivity must be between 1 and 100"
            ));
        }
        if self.loss_tangent_ppm > 1_000_000 {
            return bad(format!("{what}: loss tangent is out of range"));
        }
        Ok(())
    }
}

/// The physical stack: outer soldermask (optional), copper layers and the dielectrics between
/// them. `gaps[i]` lies between copper layer `i` and `i + 1` (front to back), top to bottom.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PhysicalStack {
    pub name: String,
    pub copper: BTreeMap<LayerId, CopperSpec>,
    pub gaps: Vec<Vec<Dielectric>>,
    #[serde(default)]
    pub top_mask: Option<Dielectric>,
    #[serde(default)]
    pub bottom_mask: Option<Dielectric>,
    /// Surface finish (informational: HASL, ENIG, …).
    #[serde(default)]
    pub finish: String,
}

const UM: Coord = 1_000;
const OZ_1: Coord = 35 * UM;
const OZ_HALF: Coord = 17_500;

fn fr4(kind: DielectricKind, t: Coord) -> Dielectric {
    Dielectric::new(kind, "FR-4", t, 4_300)
}
fn mask() -> Dielectric {
    Dielectric {
        kind: DielectricKind::SolderMask,
        material: "Solder mask".into(),
        thickness: 20 * UM,
        er_milli: 3_800,
        loss_tangent_ppm: 30_000,
        flex: false,
    }
}

impl PhysicalStack {
    fn build(name: &str, copper: &[(Coord, CopperRole)], gaps: Vec<Vec<Dielectric>>) -> Self {
        let layers = Stackup {
            copper_layers: copper.len() as u8,
        }
        .copper();
        Self {
            name: name.into(),
            copper: layers
                .into_iter()
                .zip(copper)
                .map(|(l, (t, r))| {
                    (
                        l,
                        CopperSpec {
                            thickness: *t,
                            role: *r,
                        },
                    )
                })
                .collect(),
            gaps,
            top_mask: Some(mask()),
            bottom_mask: Some(mask()),
            finish: "HASL".into(),
        }
    }

    /// A typical 1.6 mm FR-4 stack for `copper_layers` (even, 2–32): 1 oz outer, ½ oz inner
    /// copper, 0.1–0.2 mm prepregs and cores sized to reach about 1.6 mm in total.
    pub fn standard(copper_layers: u8) -> Self {
        let n = usize::from(copper_layers.clamp(2, layer::MAX_COPPER));
        let outer = (OZ_1, CopperRole::Signal);
        let inner = |i: usize| {
            (
                OZ_HALF,
                if (i % 2) == 1 {
                    CopperRole::Plane
                } else {
                    CopperRole::Signal
                },
            )
        };
        let mut copper = vec![outer];
        copper.extend((1..n - 1).map(inner));
        copper.push(outer);
        if n == 2 {
            return Self::build(
                "2 layers, 1.6 mm FR-4",
                &copper,
                vec![vec![fr4(DielectricKind::Core, 1_510 * UM)]],
            );
        }
        // n ≥ 4: alternate prepreg / core, outermost gaps are prepreg.
        let gaps_count = n - 1;
        let copper_total: Coord = copper.iter().map(|c| c.0).sum();
        let mask_total = 40 * UM;
        let target = 1_600 * UM - copper_total - mask_total;
        let prepreg_gaps = gaps_count.div_ceil(2); // gaps 0, 2, 4 …
        let core_gaps = gaps_count - prepreg_gaps;
        let prepreg_t = (2 * 100 * UM)
            .min(target / (gaps_count as Coord))
            .max(60 * UM);
        let core_t = ((target - prepreg_t * prepreg_gaps as Coord) / core_gaps.max(1) as Coord)
            .max(100 * UM);
        let gaps = (0..gaps_count)
            .map(|g| {
                if g % 2 == 0 {
                    vec![fr4(DielectricKind::Prepreg, prepreg_t)]
                } else {
                    vec![fr4(DielectricKind::Core, core_t)]
                }
            })
            .collect();
        Self::build(&format!("{n} layers, 1.6 mm FR-4"), &copper, gaps)
    }

    /// Named starting points offered by the Layer Stack Manager: `(name, copper layers)`.
    pub fn template_names() -> Vec<(&'static str, u8)> {
        vec![
            ("2 layers, 1.6 mm FR-4", 2),
            ("4 layers, 1.6 mm FR-4", 4),
            ("6 layers, 1.6 mm FR-4", 6),
            ("8 layers, 1.6 mm FR-4", 8),
            ("HDI 1+4+1 (micro-via build-up)", 6),
            ("Rigid-flex 4 layers (flex core)", 4),
        ]
    }

    pub fn template(name: &str) -> Option<Self> {
        match name {
            "2 layers, 1.6 mm FR-4" => Some(Self::standard(2)),
            "4 layers, 1.6 mm FR-4" => Some(Self::standard(4)),
            "6 layers, 1.6 mm FR-4" => Some(Self::standard(6)),
            "8 layers, 1.6 mm FR-4" => Some(Self::standard(8)),
            "HDI 1+4+1 (micro-via build-up)" => {
                // Thin build-up dielectrics (60 µm) on both outsides: micro vias span one gap.
                let mut s = Self::standard(6);
                s.name = name.into();
                let thin = |k| Dielectric::new(k, "RCC / thin prepreg", 60 * UM, 3_800);
                let build_up = || vec![fr4(DielectricKind::Prepreg, 120 * UM)];
                s.gaps[0] = vec![thin(DielectricKind::Prepreg)];
                s.gaps[1] = build_up();
                s.gaps[2] = vec![fr4(DielectricKind::Core, 1_060 * UM)];
                s.gaps[3] = build_up();
                s.gaps[4] = vec![thin(DielectricKind::Prepreg)];
                Some(s)
            }
            "Rigid-flex 4 layers (flex core)" => {
                let mut s = Self::standard(4);
                s.name = name.into();
                // The inner pair sits on a polyimide flex core with coverlay-style adhesive.
                s.gaps[1] = vec![Dielectric {
                    kind: DielectricKind::Core,
                    material: "Polyimide".into(),
                    thickness: 50 * UM,
                    er_milli: 3_400,
                    loss_tangent_ppm: 20_000,
                    flex: true,
                }];
                s.gaps[0] = vec![
                    Dielectric::new(DielectricKind::Adhesive, "Acrylic adhesive", 25 * UM, 3_500),
                    fr4(DielectricKind::Prepreg, 150 * UM),
                ];
                s.gaps[2] = s.gaps[0].iter().rev().cloned().collect();
                Some(s)
            }
            _ => None,
        }
    }

    /// Total board thickness (copper, dielectrics and mask) in nm.
    pub fn thickness(&self) -> Coord {
        self.copper.values().map(|c| c.thickness).sum::<Coord>()
            + self
                .gaps
                .iter()
                .flatten()
                .map(|d| d.thickness)
                .sum::<Coord>()
            + self.top_mask.as_ref().map_or(0, |d| d.thickness)
            + self.bottom_mask.as_ref().map_or(0, |d| d.thickness)
    }

    /// Checks the stack against the copper layers of the board.
    pub fn validate(&self, stackup: &Stackup) -> Result<(), BoardError> {
        let layers = stackup.copper();
        if self.name.chars().any(char::is_control) || self.name.len() > 96 {
            return bad("the stackup name is invalid");
        }
        if self.finish.chars().any(char::is_control) || self.finish.len() > 48 {
            return bad("the surface finish is invalid");
        }
        if self.copper.len() != layers.len() || layers.iter().any(|l| !self.copper.contains_key(l))
        {
            return bad("the stack must describe exactly the board's copper layers");
        }
        for (l, c) in &self.copper {
            if !(1_000..=1_000_000).contains(&c.thickness) {
                return bad(format!(
                    "{}: copper thickness must be between 1 µm and 1 mm",
                    layer::name(*l)
                ));
            }
        }
        if self.gaps.len() != layers.len() - 1 {
            return bad("there must be dielectric between every pair of adjacent copper layers");
        }
        for (i, gap) in self.gaps.iter().enumerate() {
            let (a, b) = (layer::name(layers[i]), layer::name(layers[i + 1]));
            if gap.is_empty() || gap.len() > 8 {
                return bad(format!(
                    "between {a} and {b}: one to eight dielectric layers"
                ));
            }
            for d in gap {
                if matches!(d.kind, DielectricKind::SolderMask) {
                    return bad(format!(
                        "between {a} and {b}: solder mask belongs on the outside"
                    ));
                }
                d.validate(&format!("between {a} and {b}"))?;
            }
        }
        for (what, m) in [("top", &self.top_mask), ("bottom", &self.bottom_mask)] {
            if let Some(m) = m {
                if m.kind != DielectricKind::SolderMask {
                    return bad(format!("the {what} outer layer must be solder mask"));
                }
                m.validate(&format!("{what} solder mask"))?;
            }
        }
        Ok(())
    }
}

impl Default for PhysicalStack {
    fn default() -> Self {
        Self::standard(2)
    }
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used)]
    use super::*;

    #[test]
    fn standard_stacks_are_valid_and_about_1_6_mm() {
        for n in (2..=layer::MAX_COPPER).step_by(2) {
            let s = PhysicalStack::standard(n);
            s.validate(&Stackup { copper_layers: n }).unwrap();
            let t = s.thickness();
            if n <= 12 {
                assert!((t - 1_600 * UM).abs() < 150 * UM, "{n}: {t}");
            }
        }
    }

    #[test]
    fn templates_validate_against_their_layer_count() {
        for (name, n) in PhysicalStack::template_names() {
            let s = PhysicalStack::template(name).unwrap();
            s.validate(&Stackup { copper_layers: n }).unwrap();
            assert_eq!(s.name, name);
        }
        assert!(PhysicalStack::template("nope").is_none());
    }

    #[test]
    fn invalid_stacks_are_refused() {
        let st = Stackup { copper_layers: 4 };
        let good = PhysicalStack::standard(4);
        assert!(good.validate(&st).is_ok());
        assert!(
            good.validate(&Stackup { copper_layers: 2 }).is_err(),
            "layer count mismatch"
        );
        let mut s = good.clone();
        s.gaps[1].clear();
        assert!(s.validate(&st).is_err(), "a gap without dielectric");
        let mut s = good.clone();
        s.gaps[0][0].thickness = 0;
        assert!(s.validate(&st).is_err());
        let mut s = good.clone();
        s.gaps[0][0].material = "  ".into();
        assert!(s.validate(&st).is_err());
        let mut s = good.clone();
        s.gaps[0][0].kind = DielectricKind::SolderMask;
        assert!(s.validate(&st).is_err());
        let mut s = good.clone();
        s.copper.get_mut(&layer::F_CU).unwrap().thickness = 5_000_000;
        assert!(s.validate(&st).is_err());
        let mut s = good;
        s.top_mask.as_mut().unwrap().kind = DielectricKind::Core;
        assert!(s.validate(&st).is_err());
    }

    #[test]
    fn hdi_and_rigid_flex_templates_use_what_they_promise() {
        let hdi = PhysicalStack::template("HDI 1+4+1 (micro-via build-up)").unwrap();
        assert_eq!(hdi.gaps[0][0].thickness, 60 * UM);
        assert!(
            (hdi.thickness() - 1_600 * UM).abs() < 20 * UM,
            "{}",
            hdi.thickness()
        );
        let rf = PhysicalStack::template("Rigid-flex 4 layers (flex core)").unwrap();
        assert!(rf.gaps.iter().flatten().any(|d| d.flex));
    }
}
