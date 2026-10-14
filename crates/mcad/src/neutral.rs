//! The neutral `.bmech` backend: deterministic, pretty-printed JSON of [`MechanicalModel`].

use crate::backend::{BackendInfo, McadBackend, McadError, Support};
use crate::model::{MechanicalModel, SCHEMA_VERSION};

pub struct NeutralBackend;

impl McadBackend for NeutralBackend {
    fn info(&self) -> BackendInfo {
        BackendInfo {
            id: "neutral",
            name: "ButureCAD neutral mechanical exchange",
            extensions: &["bmech"],
            license: "proprietary (in-house)",
            import: Support::Full,
            export: Support::Full,
            notes: "Lossless round trip of the neutral model; not a CAD interchange standard.",
        }
    }

    fn import(&self, bytes: &[u8]) -> Result<MechanicalModel, McadError> {
        let m: MechanicalModel =
            serde_json::from_slice(bytes).map_err(|e| McadError::Malformed {
                format: "bmech",
                detail: e.to_string(),
            })?;
        if m.schema_version != SCHEMA_VERSION {
            return Err(McadError::SchemaVersion {
                found: m.schema_version,
                supported: SCHEMA_VERSION,
            });
        }
        m.validate()?;
        Ok(m)
    }

    fn export(&self, model: &MechanicalModel) -> Result<Vec<u8>, McadError> {
        model.validate()?;
        let mut v = serde_json::to_vec_pretty(model).map_err(|e| McadError::Malformed {
            format: "bmech",
            detail: e.to_string(),
        })?;
        v.push(b'\n');
        Ok(v)
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;
    use crate::model::sample_model;

    #[test]
    fn round_trip_is_lossless_and_deterministic() {
        let m = sample_model();
        let a = NeutralBackend.export(&m).unwrap();
        assert_eq!(a, NeutralBackend.export(&m).unwrap());
        assert_eq!(NeutralBackend.import(&a).unwrap(), m);
    }

    #[test]
    fn malformed_and_future_files_are_errors() {
        assert!(matches!(
            NeutralBackend.import(b"{not json"),
            Err(McadError::Malformed { .. })
        ));
        let mut m = sample_model();
        m.schema_version = 99;
        let bytes = serde_json::to_vec(&m).unwrap();
        assert!(matches!(
            NeutralBackend.import(&bytes),
            Err(McadError::SchemaVersion { found: 99, .. })
        ));
    }

    #[test]
    fn invalid_models_are_not_exported() {
        let mut m = sample_model();
        m.board_thickness = -1;
        assert!(NeutralBackend.export(&m).is_err());
    }
}
