//! The Layer Stack Manager: reading and editing the physical stack of the board, templates and
//! local presets, and swapping the contents of two inner copper layers.
//!
//! Validation lives in `buturecad_pcb::PhysicalStack`; changing the number of copper layers and
//! editing the stack happen as **one** undo step.

use super::pcb_editor::{mm_str, parse_mm};
use super::*;
use buturecad_eda_core::commands::{SetPhysicalStackCommand, SetStackupCommand};
use buturecad_eda_core::model::{PcbChange, PcbObject, ViaKind};
use buturecad_libraries::{LibraryDir, StackupPreset};
use buturecad_pcb::{
    layer, CopperRole, CopperSpec, Dielectric, DielectricKind, LayerId, PhysicalStack, Stackup,
};

fn num(v: u32, scale: f64) -> String {
    let s = format!("{:.3}", f64::from(v) / scale);
    let s = s.trim_end_matches('0').trim_end_matches('.');
    if s.is_empty() {
        "0".into()
    } else {
        s.to_owned()
    }
}

fn diel_row(d: &Dielectric) -> DielRow {
    DielRow {
        kind: d.kind.id().into(),
        material: d.material.clone(),
        thickness: mm_str(d.thickness),
        er: num(d.er_milli, 1000.0),
        loss_tangent: num(d.loss_tangent_ppm, 1_000_000.0),
        flex: d.flex,
    }
}

fn parse_diel(r: &DielRow) -> Result<Dielectric, String> {
    let kind = DielectricKind::parse(r.kind.trim()).ok_or("unknown dielectric type")?;
    let f = |s: &str, what: &str, scale: f64| -> Result<u32, String> {
        let v: f64 = s
            .trim()
            .replace(',', ".")
            .parse()
            .map_err(|_| format!("{what} must be a number"))?;
        if !v.is_finite() || !(0.0..=1.0e6).contains(&v) {
            return Err(format!("{what} is out of range"));
        }
        Ok((v * scale).round() as u32)
    };
    Ok(Dielectric {
        kind,
        material: r.material.trim().to_owned(),
        thickness: parse_mm(&r.thickness, "dielectric thickness")?,
        er_milli: f(&r.er, "relative permittivity", 1000.0)?,
        loss_tangent_ppm: f(&r.loss_tangent, "loss tangent", 1_000_000.0)?,
        flex: r.flex,
    })
}

fn info_of(stack: &PhysicalStack, stackup: &Stackup) -> StackupInfo {
    StackupInfo {
        name: stack.name.clone(),
        copper_layers: u32::from(stackup.copper_layers),
        finish: stack.finish.clone(),
        total_thickness: mm_str(stack.thickness()),
        copper: stackup
            .copper()
            .into_iter()
            .filter_map(|l| {
                let c = stack.copper.get(&l)?;
                Some(CopperRow {
                    layer: layer::name(l),
                    layer_id: u32::from(l.0),
                    thickness: mm_str(c.thickness),
                    role: c.role.id().into(),
                })
            })
            .collect(),
        gaps: stack
            .gaps
            .iter()
            .map(|g| g.iter().map(diel_row).collect())
            .collect(),
        top_mask: stack.top_mask.as_ref().map(diel_row),
        bottom_mask: stack.bottom_mask.as_ref().map(diel_row),
        templates: PhysicalStack::template_names()
            .into_iter()
            .map(|(n, c)| TemplateInfo {
                name: n.into(),
                copper_layers: u32::from(c),
                scope: "built-in".into(),
                error: None,
            })
            .collect(),
        presets: Vec::new(),
    }
}

fn stack_of(info: &StackupInfo) -> Result<(Stackup, PhysicalStack), String> {
    let copper_layers = u8::try_from(info.copper_layers).map_err(|_| "bad layer count")?;
    let stackup = Stackup { copper_layers };
    if !stackup.is_valid() {
        return Err("copper layers must be an even number from 2 to 32".into());
    }
    let layers = stackup.copper();
    if info.copper.len() != layers.len() {
        return Err("the stack must list every copper layer".into());
    }
    let mut copper = std::collections::BTreeMap::new();
    for (l, row) in layers.iter().zip(&info.copper) {
        copper.insert(
            *l,
            CopperSpec {
                thickness: parse_mm(&row.thickness, "copper thickness")?,
                role: CopperRole::parse(row.role.trim()).ok_or("role is signal, plane or mixed")?,
            },
        );
    }
    let mut gaps = Vec::new();
    for g in &info.gaps {
        gaps.push(g.iter().map(parse_diel).collect::<Result<Vec<_>, _>>()?);
    }
    let mask = |m: &Option<DielRow>| m.as_ref().map(parse_diel).transpose();
    let stack = PhysicalStack {
        name: info.name.trim().to_owned(),
        copper,
        gaps,
        top_mask: mask(&info.top_mask)?,
        bottom_mask: mask(&info.bottom_mask)?,
        finish: info.finish.trim().to_owned(),
    };
    stack.validate(&stackup).map_err(|e| e.to_string())?;
    Ok((stackup, stack))
}

impl Workspace {
    fn preset_dir(&self, scope: &str) -> Result<LibraryDir, String> {
        self.library_dirs()
            .into_iter()
            .find(|(s, _)| *s == scope)
            .map(|(_, d)| d)
            .ok_or_else(|| format!("there is no {scope} library folder"))
    }

    /// The current stack plus the templates and local presets the manager offers.
    pub fn stackup_info(&self) -> Result<StackupInfo, String> {
        let doc = self.pcb_doc().ok_or("no project is open")?;
        let mut info = info_of(&doc.physical, &doc.stackup);
        for (scope, dir) in self.library_dirs() {
            for s in dir.list_stackup_presets() {
                info.presets.push(TemplateInfo {
                    name: s.name,
                    copper_layers: s.item_count as u32,
                    scope: scope.into(),
                    error: s.error,
                });
            }
        }
        Ok(info)
    }

    /// The standard stack for `copper_layers`, or a named built-in template (for the dialog).
    pub fn stackup_template(
        &self,
        template: &str,
        copper_layers: u32,
    ) -> Result<StackupInfo, String> {
        let (stackup, stack) = if template.is_empty() {
            let n = u8::try_from(copper_layers).map_err(|_| "bad layer count")?;
            let st = Stackup { copper_layers: n };
            if !st.is_valid() {
                return Err("copper layers must be an even number from 2 to 32".into());
            }
            (st, PhysicalStack::standard(n))
        } else {
            let stack = PhysicalStack::template(template).ok_or("unknown template")?;
            let n = u8::try_from(stack.copper.len()).map_err(|_| "bad template")?;
            (Stackup { copper_layers: n }, stack)
        };
        Ok(info_of(&stack, &stackup))
    }

    pub fn stackup_preset_load(&self, scope: &str, name: &str) -> Result<StackupInfo, String> {
        let p = self
            .preset_dir(scope)?
            .load_stackup_preset(name)
            .map_err(|e| e.to_string())?;
        Ok(info_of(
            &p.stack,
            &Stackup {
                copper_layers: p.copper_layers,
            },
        ))
    }

    pub fn stackup_preset_save(
        &mut self,
        scope: &str,
        name: &str,
        info: StackupInfo,
    ) -> Result<(), String> {
        let (stackup, mut stack) = stack_of(&info)?;
        stack.name = name.trim().to_owned();
        let preset = StackupPreset::new(name.trim(), stackup.copper_layers, stack)
            .map_err(|e| e.to_string())?;
        self.preset_dir(scope)?
            .save_stackup_preset(&preset)
            .map_err(|e| e.to_string())?;
        self.log(
            MessageLevel::Info,
            format!("Saved the layer stack preset “{}” ({scope})", preset.name),
        );
        Ok(())
    }

    pub fn stackup_preset_delete(&mut self, scope: &str, name: &str) -> Result<(), String> {
        self.preset_dir(scope)?
            .delete_stackup_preset(name)
            .map_err(|e| e.to_string())
    }

    /// Applies an edited stack (and layer count) as one undo step.
    pub fn stackup_apply(&mut self, info: StackupInfo) -> Result<(), String> {
        let (stackup, stack) = stack_of(&info)?;
        let open = self.open.as_mut().ok_or("no project is open")?;
        let pcb = open.engine.project().pcb();
        if pcb.stackup == stackup && pcb.physical == stack {
            return Ok(());
        }
        let (before_stackup, before_stack) = (pcb.stackup, pcb.physical.clone());
        open.engine.begin_transaction("Edit layer stack");
        let mut result = Ok(());
        if before_stackup != stackup {
            result = open
                .engine
                .execute(SetStackupCommand::new(before_stackup, stackup));
        }
        if result.is_ok() {
            let current = open.engine.project().pcb().physical.clone();
            result = open.engine.execute(SetPhysicalStackCommand::new(
                "Edit layer stack",
                if before_stackup != stackup {
                    current
                } else {
                    before_stack
                },
                stack,
            ));
        }
        match result {
            Ok(()) => open
                .engine
                .commit_transaction()
                .map_err(|e| e.to_string())?,
            Err(e) => {
                let _ = open.engine.rollback_transaction();
                return Err(e.to_string());
            }
        }
        self.built_for = None;
        if !self
            .pcb_doc()
            .is_some_and(|d| d.stackup.has_copper(LayerId(self.active_pcb_layer)))
        {
            self.active_pcb_layer = 0;
        }
        self.command_log.push("Edit layer stack".into());
        Ok(())
    }

    /// Exchanges everything on inner copper layers `a` and `b` (tracks, zones, keepout layer
    /// lists): the physical layers keep their thickness, the design moves. Through vias are
    /// unaffected; blind and buried vias make the swap impossible.
    pub fn stackup_swap_inner(&mut self, a: LayerId, b: LayerId) -> Result<(), String> {
        let doc = self.pcb_doc().ok_or("no project is open")?;
        let inner = |l: LayerId| doc.stackup.has_copper(l) && l != layer::F_CU && l != layer::B_CU;
        if !inner(a) || !inner(b) || a == b {
            return Err("pick two different inner copper layers".into());
        }
        if doc
            .vias
            .values()
            .any(|v| !matches!(v.kind, ViaKind::Through))
        {
            return Err(
                "blind and buried vias depend on the layer order; remove them first".into(),
            );
        }
        let swap = |l: LayerId| {
            if l == a {
                b
            } else if l == b {
                a
            } else {
                l
            }
        };
        let mut changes: Vec<PcbChange> = Vec::new();
        for (id, t) in &doc.tracks {
            if swap(t.layer) != t.layer {
                let mut n = t.clone();
                n.layer = swap(t.layer);
                changes.push(PcbChange::modify(
                    *id,
                    PcbObject::Track(t.clone()),
                    PcbObject::Track(n),
                ));
            }
        }
        for (id, z) in &doc.zones {
            if swap(z.layer) != z.layer {
                let mut n = z.clone();
                n.layer = swap(z.layer);
                n.fill = None;
                changes.push(PcbChange::modify(
                    *id,
                    PcbObject::Zone(z.clone()),
                    PcbObject::Zone(n),
                ));
            }
        }
        for (id, k) in &doc.keepouts {
            if k.layers.iter().any(|l| swap(*l) != *l) {
                let mut n = k.clone();
                n.layers = k.layers.iter().map(|l| swap(*l)).collect();
                changes.push(PcbChange::modify(
                    *id,
                    PcbObject::Keepout(k.clone()),
                    PcbObject::Keepout(n),
                ));
            }
        }
        if changes.is_empty() {
            return Err("there is nothing on those layers to swap".into());
        }
        if self.pcb_commit("Swap inner layers", changes) {
            Ok(())
        } else {
            Err("the swap was rejected (see Messages)".into())
        }
    }
}
