//! The STEP backend slot.
//!
//! Reading the ISO 10303-21 structure is implemented ([`crate::part21`]); **interpreting** AP203/
//! AP214/AP242 geometry (B-rep, assemblies, placements) and **writing** STEP are not, because they
//! need a geometry kernel whose licence has not been cleared (see `docs/mcad.md`). Import and
//! export therefore return [`McadError::Unsupported`] instead of pretending.

use crate::backend::{BackendInfo, McadBackend, McadError, Support};
use crate::model::MechanicalModel;
use crate::part21::{self, ApplicationProtocol};

pub struct StepBackend;

/// What a STEP file contains, as far as the syntax tells.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StepSummary {
    pub protocol: ApplicationProtocol,
    pub schemas: Vec<String>,
    pub entities: usize,
    pub solids: usize,
    pub shells: usize,
    pub products: usize,
}

impl StepBackend {
    /// Parses the file structure and summarises it. No geometry is interpreted.
    pub fn inspect(bytes: &[u8]) -> Result<StepSummary, McadError> {
        let text = std::str::from_utf8(bytes).map_err(|e| McadError::Malformed {
            format: "STEP",
            detail: format!("not UTF-8/ASCII text: {e}"),
        })?;
        let p = part21::parse(text)?;
        Ok(StepSummary {
            protocol: p.protocol(),
            schemas: p.schemas.clone(),
            entities: p.entities.len(),
            solids: p.count("MANIFOLD_SOLID_BREP") + p.count("BREP_WITH_VOIDS"),
            shells: p.count("CLOSED_SHELL") + p.count("OPEN_SHELL"),
            products: p.count("PRODUCT"),
        })
    }
}

impl McadBackend for StepBackend {
    fn info(&self) -> BackendInfo {
        BackendInfo {
            id: "step",
            name: "STEP (ISO 10303-21)",
            extensions: &["step", "stp"],
            license: "none linked (syntax reader is in-house)",
            import: Support::None,
            export: Support::None,
            notes: "File structure can be inspected; geometry import/export needs a kernel (not selected).",
        }
    }

    fn import(&self, bytes: &[u8]) -> Result<MechanicalModel, McadError> {
        let s = Self::inspect(bytes)?;
        Err(McadError::Unsupported {
            backend: "step",
            what: "STEP geometry import",
            detail: format!(
                "valid Part 21 file ({:?}, {} entities, {} solids) but its geometry is not interpreted",
                s.protocol, s.entities, s.solids
            ),
        })
    }

    fn export(&self, _model: &MechanicalModel) -> Result<Vec<u8>, McadError> {
        Err(McadError::Unsupported {
            backend: "step",
            what: "STEP export",
            detail: "no geometry kernel is linked; use the neutral .bmech format".into(),
        })
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;
    use crate::backend::Registry;

    const FILE: &[u8] = b"ISO-10303-21;\nHEADER;\nFILE_SCHEMA(('CONFIG_CONTROL_DESIGN'));\nENDSEC;\nDATA;\n#1 = PRODUCT('a','a','',());\n#2 = MANIFOLD_SOLID_BREP('b',#3);\nENDSEC;\nEND-ISO-10303-21;\n";

    #[test]
    fn inspects_but_does_not_import() {
        let s = StepBackend::inspect(FILE).unwrap();
        assert_eq!(s.protocol, ApplicationProtocol::Ap203);
        assert_eq!((s.products, s.solids), (1, 1));
        assert!(matches!(
            StepBackend.import(FILE),
            Err(McadError::Unsupported { .. })
        ));
        assert!(matches!(
            StepBackend.import(b"garbage"),
            Err(McadError::Malformed { .. })
        ));
    }

    #[test]
    fn registry_routes_by_extension_and_reports_capabilities() {
        let r = Registry::with_builtin();
        assert_eq!(r.by_extension(".STP").unwrap().info().id, "step");
        assert_eq!(r.by_extension("bmech").unwrap().info().id, "neutral");
        assert!(matches!(
            r.by_extension("iges"),
            Err(McadError::NoBackend(_))
        ));
        let step = r.infos().into_iter().find(|i| i.id == "step").unwrap();
        assert_eq!((step.import, step.export), (Support::None, Support::None));
    }
}
