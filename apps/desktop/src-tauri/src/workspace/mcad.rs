//! Bridge from the board (`PcbDoc`) to the neutral mechanical model of `buturecad-mcad`.
//!
//! The PCB core and the STEP/MCAD layer do not know each other: this module is the only place
//! that converts between them (export direction only; importing mechanical data into the board
//! will have to go through the Command Engine). Nothing is invented: a component gets geometry
//! only from the 3D models its footprint references, because the board stores no body heights.

use buturecad_eda_core::model::PcbDoc;
use buturecad_eda_core::outline::outline_loops;
use buturecad_geometry::Point;
use buturecad_mcad as mcad;
use buturecad_pcb::PadKind;

fn pt(p: Point) -> mcad::Pt {
    mcad::Pt { x: p.x, y: p.y }
}

/// Builds the mechanical model of the board: outline loops, non-plated and plated drilled holes
/// that are mounting holes, components with referenced 3D models, and locked components as
/// fixed-position constraints.
pub(crate) fn build_mechanical(doc: &PcbDoc, name: &str) -> mcad::MechanicalModel {
    let mut m = mcad::MechanicalModel::new(name, doc.physical.thickness());
    m.outline = outline_loops(doc)
        .iter()
        .map(|l| l.iter().map(|p| pt(*p)).collect())
        .collect();
    for f in doc.placed.values() {
        let Some(def) = doc.footprints.get(&f.footprint) else {
            continue;
        };
        let mut n = 0;
        for (wp, dp) in f.world_pads(def).iter().zip(&def.pads) {
            if let (PadKind::NonPlated, Some(d)) = (wp.kind, dp.drill) {
                n += 1;
                m.holes.push(mcad::MountingHole {
                    id: format!("{}.H{n}", f.reference),
                    center: pt(wp.center),
                    diameter: d.diameter,
                    plated: false,
                });
            }
        }
        m.components.push(mcad::Component {
            reference: f.reference.clone(),
            footprint: f.footprint.clone(),
            position: pt(f.placement.position),
            rotation_mdeg: i32::from(f.placement.rotation % 4) * 90_000,
            side: if f.placement.flipped {
                mcad::Side::Bottom
            } else {
                mcad::Side::Top
            },
            bodies: def
                .models
                .iter()
                .map(|r| {
                    mcad::Body::External(mcad::ModelRef {
                        path: r.path.clone(),
                        offset: r.offset,
                        rotation_mdeg: r.rotation_mdeg,
                        scale_ppm: r.scale_ppm,
                    })
                })
                .collect(),
        });
        if f.locked {
            m.constraints.push(mcad::Constraint::FixedComponent {
                reference: f.reference.clone(),
            });
        }
    }
    m
}

impl super::Workspace {
    /// Exports the mechanical model of the open board to `path`; the backend is chosen by the
    /// file extension. STEP is reported as unsupported (see `docs/mcad.md`), never faked.
    pub fn mcad_export(&mut self, path: &str) -> Result<(), String> {
        let open = self.open.as_ref().ok_or("no project is open")?;
        let ext = std::path::Path::new(path)
            .extension()
            .and_then(|e| e.to_str())
            .unwrap_or("");
        let registry = mcad::Registry::with_builtin();
        let backend = registry.by_extension(ext).map_err(|e| e.to_string())?;
        let model = build_mechanical(open.engine.project().pcb(), open.engine.project().name());
        let bytes = backend.export(&model).map_err(|e| e.to_string())?;
        buturecad_io::atomic::atomic_write(std::path::Path::new(path), &bytes)
            .map_err(|e| e.to_string())?;
        self.log(
            super::MessageLevel::Info,
            format!(
                "Exported the mechanical model ({} components, {} holes) to {path}",
                model.components.len(),
                model.holes.len()
            ),
        );
        Ok(())
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;
    use buturecad_eda_core::model::Id;
    use buturecad_mcad::McadBackend;
    use buturecad_pcb::generators::Soic;
    use buturecad_pcb::{Model3d, PlacedFootprint, Placement};
    use std::collections::BTreeMap;

    fn doc() -> PcbDoc {
        let mut d = PcbDoc::default();
        let mut def = Soic::narrow(8).generate().unwrap();
        def.models.push(Model3d::new("models/soic8.step"));
        let name = def.name.clone();
        d.footprints.insert(name.clone(), def);
        d.placed.insert(
            Id(10),
            PlacedFootprint {
                footprint: name,
                reference: "U1".into(),
                value: String::new(),
                placement: Placement::at(Point::new(5_000_000, 5_000_000)),
                locked: true,
                link: None,
                pad_nets: BTreeMap::new(),
            },
        );
        d
    }

    #[test]
    fn components_models_and_locks_are_exported_without_invented_geometry() {
        let m = build_mechanical(&doc(), "demo");
        assert_eq!(m.components.len(), 1);
        assert!(matches!(
            m.components[0].bodies.as_slice(),
            [mcad::Body::External(r)] if r.path == "models/soic8.step"
        ));
        assert_eq!(
            m.constraints,
            vec![mcad::Constraint::FixedComponent {
                reference: "U1".into()
            }]
        );
    }

    #[test]
    fn a_board_without_a_closed_outline_is_not_exported() {
        let m = build_mechanical(&doc(), "demo");
        assert!(m.outline.is_empty());
        assert!(mcad::NeutralBackend.export(&m).is_err());
    }
}
