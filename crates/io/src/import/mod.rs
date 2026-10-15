//! Importers: turning other tools' files into ButureCAD designs (roadmap stage 43).
//!
//! Architecture:
//!
//! ```text
//! file ─▶ Importer (one per format, in this folder) ─▶ ImportOutcome { Design (EDA IR), extra symbols, ImportReport }
//!                                                          └▶ the same ir_apply path as ButureScript → project
//! ```
//!
//! * An importer implements [`Importer`]; formats are registered in [`Registry`]. Adding a format
//!   means adding a file here, nothing else changes.
//! * **Nothing is silently dropped.** Every record the importer reads is counted with
//!   [`ImportReport::seen`] and must end up *converted*, *ignored* (with the reason) or
//!   *unsupported* (with the reason). [`ImportReport::finish`] turns any unaccounted record into an
//!   error, so a forgetful importer is caught by the report itself.
//! * **Licensing:** importers are written from the published *format* descriptions only — no code
//!   of other tools is copied (CLAUDE.md §31). Each importer states its sources in
//!   [`Importer::basis`]. Formats whose documentation or licence does not allow an independent
//!   implementation are not implemented.

pub mod kicad_netlist;
pub mod report;
pub mod spice;

use buturecad_eda_core::ir::Design;
use buturecad_schematic::Symbol;
pub use report::{ImportReport, ReportItem};

/// What an importer produced. `design` is `None` when the file could not be converted at all.
#[derive(Debug, Clone)]
pub struct ImportOutcome {
    pub design: Option<Design>,
    /// Symbols the design uses that do not exist in the built-in set (generated from the file).
    pub symbols: Vec<Symbol>,
    pub report: ImportReport,
}

pub trait Importer {
    fn id(&self) -> &'static str;
    fn name(&self) -> &'static str;
    fn extensions(&self) -> &'static [&'static str];
    /// What this importer is based on, and its licensing basis, in words.
    fn basis(&self) -> &'static str;
    /// Does the text look like this format?
    fn sniff(&self, text: &str) -> bool;
    fn import(&self, text: &str, source_name: &str) -> ImportOutcome;
}

#[derive(Debug, thiserror::Error)]
pub enum ImportError {
    #[error("unknown import format “{0}” (known: {1})")]
    UnknownFormat(String, String),
    #[error("cannot tell which format “{0}” is; pass --format ({1})")]
    CannotDetect(String, String),
    #[error("the file is not valid UTF-8 text")]
    NotText,
}

pub struct Registry {
    importers: Vec<Box<dyn Importer>>,
}

impl Registry {
    pub fn builtin() -> Self {
        Self {
            importers: vec![
                Box::new(kicad_netlist::KicadNetlist),
                Box::new(spice::SpiceNetlist),
            ],
        }
    }

    pub fn register(&mut self, i: Box<dyn Importer>) {
        self.importers.push(i);
    }

    pub fn list(&self) -> Vec<(&'static str, &'static str, &'static [&'static str])> {
        self.importers
            .iter()
            .map(|i| (i.id(), i.name(), i.extensions()))
            .collect()
    }

    fn ids(&self) -> String {
        self.importers
            .iter()
            .map(|i| i.id())
            .collect::<Vec<_>>()
            .join(", ")
    }

    /// Picks the importer by `format` id, else by content, else by extension, and runs it.
    pub fn import(
        &self,
        bytes: &[u8],
        file_name: &str,
        format: Option<&str>,
    ) -> Result<(ImportOutcome, &'static str), ImportError> {
        let text = std::str::from_utf8(bytes).map_err(|_| ImportError::NotText)?;
        let text = text.strip_prefix('\u{feff}').unwrap_or(text);
        let importer = match format {
            Some(f) if f != "auto" => self
                .importers
                .iter()
                .find(|i| i.id() == f)
                .ok_or_else(|| ImportError::UnknownFormat(f.to_owned(), self.ids()))?,
            _ => {
                let ext = file_name
                    .rsplit('.')
                    .next()
                    .unwrap_or("")
                    .to_ascii_lowercase();
                let by_content: Vec<_> = self.importers.iter().filter(|i| i.sniff(text)).collect();
                match by_content.as_slice() {
                    [one] => one,
                    _ => self
                        .importers
                        .iter()
                        .find(|i| {
                            i.extensions().contains(&ext.as_str())
                                && (by_content.is_empty()
                                    || by_content.iter().any(|b| b.id() == i.id()))
                        })
                        .ok_or_else(|| {
                            ImportError::CannotDetect(file_name.to_owned(), self.ids())
                        })?,
                }
            }
        };
        Ok((importer.import(text, file_name), importer.id()))
    }
}

/// A project name made from a file name.
pub fn name_from_file(file_name: &str) -> String {
    let stem = std::path::Path::new(file_name)
        .file_stem()
        .and_then(|s| s.to_str())
        .unwrap_or("imported");
    let n: String = stem
        .chars()
        .map(|c| {
            if c.is_alphanumeric() || c == '_' || c == '-' {
                c
            } else {
                '_'
            }
        })
        .collect();
    if n.is_empty() {
        "imported".into()
    } else {
        n
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;

    #[test]
    fn the_registry_detects_the_format_and_rejects_unknown_ones() {
        let r = Registry::builtin();
        let (o, id) = r
            .import(b"title\nR1 a b 1k\n.end\n", "x.cir", None)
            .unwrap();
        assert_eq!((id, o.design.is_some()), ("spice", true));
        let (_, id) = r
            .import(
                b"(export (version \"E\") (components) (nets))",
                "y.net",
                None,
            )
            .unwrap();
        assert_eq!(id, "kicad-netlist");
        assert!(matches!(
            r.import(b"nothing useful", "z.dat", None),
            Err(ImportError::CannotDetect(..))
        ));
        assert!(matches!(
            r.import(b"x", "z.dat", Some("altium")),
            Err(ImportError::UnknownFormat(..))
        ));
        assert!(matches!(
            r.import(&[0xff, 0xfe, 0x00], "z.net", None),
            Err(ImportError::NotText)
        ));
        assert_eq!(r.list().len(), 2);
        assert_eq!(name_from_file("My Board v2.net"), "My_Board_v2");
    }

    /// An importer that forgets to account for a record is caught by its own report.
    #[test]
    fn a_forgetful_importer_is_detected() {
        struct Sloppy;
        impl Importer for Sloppy {
            fn id(&self) -> &'static str {
                "sloppy"
            }
            fn name(&self) -> &'static str {
                "sloppy"
            }
            fn extensions(&self) -> &'static [&'static str] {
                &["x"]
            }
            fn basis(&self) -> &'static str {
                "test"
            }
            fn sniff(&self, _: &str) -> bool {
                true
            }
            fn import(&self, _: &str, s: &str) -> ImportOutcome {
                let mut r = ImportReport::new("sloppy", s);
                r.seen(2);
                r.converted("thing", "a", "");
                ImportOutcome {
                    design: None,
                    symbols: Vec::new(),
                    report: r.finish(),
                }
            }
        }
        let mut reg = Registry {
            importers: Vec::new(),
        };
        reg.register(Box::new(Sloppy));
        let (o, _) = reg.import(b"x", "a.x", Some("sloppy")).unwrap();
        assert!(o.report.errors[0].contains("information may have been dropped"));
    }
}
