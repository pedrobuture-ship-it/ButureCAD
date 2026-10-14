//! `plugin.json`.

use crate::PluginError;
use serde::{Deserialize, Serialize};
use std::collections::BTreeSet;

pub const FORMAT_TAG: &str = "buturecad-plugin";
/// Version of the host ABI described in `docs/plugin-sdk/`.
pub const API_VERSION: u32 = 1;
pub const MAX_WASM_BYTES: u64 = 16 * 1024 * 1024;

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub enum Category {
    ComponentProvider,
    SymbolProvider,
    FootprintProvider,
    Model3dProvider,
    Importer,
    Exporter,
    RulePlugin,
    ToolPlugin,
    SimulationPlugin,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum Permission {
    Filesystem,
    Network,
    ProjectRead,
    ProjectWrite,
}

impl Permission {
    pub fn name(self) -> &'static str {
        match self {
            Permission::Filesystem => "filesystem",
            Permission::Network => "network",
            Permission::ProjectRead => "project-read",
            Permission::ProjectWrite => "project-write",
        }
    }

    pub fn parse(s: &str) -> Option<Self> {
        Some(match s {
            "filesystem" => Permission::Filesystem,
            "network" => Permission::Network,
            "project-read" => Permission::ProjectRead,
            "project-write" => Permission::ProjectWrite,
            _ => return None,
        })
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Manifest {
    pub format: String,
    pub api: u32,
    pub id: String,
    pub name: String,
    pub version: String,
    #[serde(default)]
    pub description: String,
    pub categories: BTreeSet<Category>,
    /// Permissions the plugin asks for. Nothing is granted until the user grants it.
    #[serde(default)]
    pub permissions: BTreeSet<Permission>,
    /// File name of the module next to the manifest.
    pub wasm: String,
}

impl Manifest {
    pub fn parse(bytes: &[u8]) -> Result<Self, PluginError> {
        let m: Manifest =
            serde_json::from_slice(bytes).map_err(|e| PluginError::Manifest(e.to_string()))?;
        m.validate()?;
        Ok(m)
    }

    pub fn validate(&self) -> Result<(), PluginError> {
        let bad = |s: &str| Err(PluginError::Manifest(s.to_owned()));
        if self.format != FORMAT_TAG {
            return bad("format must be “buturecad-plugin”");
        }
        if self.api != API_VERSION {
            return Err(PluginError::Manifest(format!(
                "plugin API {} is not supported (this build speaks API {API_VERSION})",
                self.api
            )));
        }
        let id_ok = !self.id.is_empty()
            && self.id.len() <= 64
            && self.id.chars().all(|c| {
                c.is_ascii_lowercase() || c.is_ascii_digit() || matches!(c, '.' | '-' | '_')
            })
            && !self.id.starts_with('.');
        if !id_ok {
            return bad("id must be 1–64 characters of a-z 0-9 . - _ and not start with a dot");
        }
        if self.name.trim().is_empty()
            || self.name.len() > 128
            || self.name.chars().any(char::is_control)
        {
            return bad("name must be 1–128 printable characters");
        }
        if self.version.is_empty()
            || self.version.len() > 32
            || self.version.chars().any(char::is_control)
        {
            return bad("version is not valid");
        }
        if self.description.len() > 2000 {
            return bad("description is too long");
        }
        if self.categories.is_empty() {
            return bad("declare at least one category");
        }
        if self.wasm.is_empty()
            || self.wasm.contains(['/', '\\'])
            || self.wasm.starts_with('.')
            || !self.wasm.ends_with(".wasm")
        {
            return bad("wasm must be a plain file name ending in .wasm, next to plugin.json");
        }
        Ok(())
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;

    pub(crate) fn json(extra: &str) -> String {
        format!(
            r#"{{"format":"buturecad-plugin","api":1,"id":"acme.parts","name":"Acme","version":"1.0.0","categories":["ComponentProvider"],"wasm":"plugin.wasm"{extra}}}"#
        )
    }

    #[test]
    fn valid_and_invalid_manifests() {
        let m = Manifest::parse(json(r#","permissions":["network","project-read"]"#).as_bytes())
            .unwrap();
        assert!(m.permissions.contains(&Permission::Network));
        assert_eq!(
            Permission::parse("project-read"),
            Some(Permission::ProjectRead)
        );
        for bad in [
            json("").replace("acme.parts", "../x"),
            json("").replace("plugin.wasm", "../plugin.wasm"),
            json("").replace("plugin.wasm", "plugin.txt"),
            json("").replace("\"api\":1", "\"api\":9"),
            json("").replace("[\"ComponentProvider\"]", "[]"),
            json("").replace("[\"ComponentProvider\"]", "[\"Nope\"]"),
            json(r#","permissions":["root"]"#),
            "{".to_string(),
        ] {
            assert!(Manifest::parse(bad.as_bytes()).is_err(), "{bad}");
        }
    }
}
