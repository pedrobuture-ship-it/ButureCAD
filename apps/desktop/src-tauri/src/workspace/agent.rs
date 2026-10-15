//! AI Workspace: requests become proposals, proposals are reviewed, approved ones go through the
//! Command Engine.
//!
//! The Core contains no model. A *provider* (`buturecad_agent_api::AgentProvider`) turns the
//! request into drafts; the Agent API validates them on a copy and produces the diff and the
//! ERC/DRC comparison; only the buttons here (Apply / Reject) can change the project, and Apply is
//! one undoable step. Every call is appended to `<project>/.buture/agent-audit.jsonl`.

use super::*;
use buturecad_agent_api::{
    builtin_providers, AgentApi, AgentProvider, AgentSurface, ProposalStatus, ProviderKind,
    Reviewer,
};
use std::io::Write as _;

pub(crate) struct AgentState {
    api: AgentApi,
    providers: Vec<Box<dyn AgentProvider>>,
    provider: String,
    selected: Option<String>,
    error: String,
    persisted: usize,
    rationale: std::collections::BTreeMap<String, Vec<String>>,
}

impl Default for AgentState {
    fn default() -> Self {
        Self {
            api: AgentApi::new(Box::new(buturecad_scripting::resolve_footprint)),
            providers: builtin_providers(),
            provider: "decoupling".into(),
            selected: None,
            error: String::new(),
            persisted: 0,
            rationale: Default::default(),
        }
    }
}

const ACTOR: &str = "user";

impl Workspace {
    fn agent_persist(&mut self) {
        let Some(open) = self.open.as_ref() else {
            return;
        };
        let dir = open.root.join(buturecad_io::session::STATE_DIR);
        let entries = self.agent.api.audit.entries();
        if self.agent.persisted >= entries.len() {
            return;
        }
        let text: String = entries[self.agent.persisted..]
            .iter()
            .filter_map(|e| serde_json::to_string(e).ok())
            .map(|l| l + "\n")
            .collect();
        let ok = std::fs::create_dir_all(&dir)
            .and_then(|()| {
                std::fs::OpenOptions::new()
                    .create(true)
                    .append(true)
                    .open(dir.join("agent-audit.jsonl"))
            })
            .and_then(|mut f| f.write_all(text.as_bytes()));
        if let Err(e) = ok {
            self.log(
                MessageLevel::Warn,
                format!("Cannot write the agent audit log: {e}"),
            );
        } else {
            self.agent.persisted = self.agent.api.audit.entries().len();
        }
    }

    /// `provider <id>`, `request <text>`, `select <id>`, `apply <id>`, `reject <id>`.
    pub fn agent_command(&mut self, name: &str, arg: Option<&str>) -> Result<(), String> {
        let a = arg.unwrap_or("").to_owned();
        let r = self.agent_command_inner(name, &a);
        self.agent_persist();
        r
    }

    fn agent_command_inner(&mut self, name: &str, a: &str) -> Result<(), String> {
        match name {
            "provider" => {
                if !self.agent.providers.iter().any(|p| p.id() == a) {
                    return Err("unknown provider".into());
                }
                self.agent.provider = a.to_owned();
                self.agent.error.clear();
                Ok(())
            }
            "select" => {
                if self.agent.api.proposal(a).is_none() {
                    return Err("no such proposal".into());
                }
                self.agent.selected = Some(a.to_owned());
                Ok(())
            }
            "request" => {
                self.agent.error.clear();
                let open = self.open.as_ref().ok_or("no project is open")?;
                let revision = open.engine.revision();
                let provider = self
                    .agent
                    .providers
                    .iter()
                    .find(|p| p.id() == self.agent.provider)
                    .ok_or("no provider selected")?;
                let (pid, pname) = (provider.id(), provider.name());
                let drafts = match provider.draft(a, open.engine.project()) {
                    Ok(d) => d,
                    Err(e) => {
                        self.agent.error = e.clone();
                        return Err(e);
                    }
                };
                let project = open.engine.project().clone();
                let mut last = None;
                for d in drafts {
                    let p = self
                        .agent
                        .api
                        .propose(&project, revision, &d.title, d.actions, pname);
                    self.agent.rationale.insert(p.id.clone(), d.rationale);
                    last = Some(p.id);
                }
                self.agent.selected = last;
                self.log(
                    MessageLevel::Info,
                    format!("AI Workspace: {pid} produced a proposal; review it before applying"),
                );
                Ok(())
            }
            "apply" => {
                let open = self.open.as_mut().ok_or("no project is open")?;
                self.agent
                    .api
                    .apply(&mut open.engine, a, ACTOR)
                    .map_err(|e| e.to_string())?;
                self.built_for = None;
                self.command_log.push(format!("Apply agent proposal {a}"));
                self.log(
                    MessageLevel::Info,
                    format!("Applied proposal {a} (one undo step)"),
                );
                Ok(())
            }
            "reject" => {
                self.agent
                    .api
                    .reject(a, ACTOR, "rejected in the AI Workspace")
                    .map_err(|e| e.to_string())?;
                Ok(())
            }
            other => Err(format!("unknown AI Workspace action: {other}")),
        }
    }

    pub(crate) fn agent_info(&self) -> AgentInfo {
        let revision = self.open.as_ref().map(|o| o.engine.revision());
        let delta = |d: &Option<buturecad_agent_api::api::Delta>| {
            d.as_ref().map(|d| AgentDeltaInfo {
                errors_before: d.errors_before as u32,
                errors_after: d.errors_after as u32,
                warnings_before: d.warnings_before as u32,
                warnings_after: d.warnings_after as u32,
                resolved: d.resolved as u32,
                new: d
                    .new
                    .iter()
                    .map(|f| format!("{}[{}]: {}", f.severity, f.rule, f.message))
                    .collect(),
            })
        };
        let proposals: Vec<AgentProposalInfo> = self
            .agent
            .api
            .proposals()
            .into_iter()
            .rev()
            .map(|p| AgentProposalInfo {
                id: p.id.clone(),
                title: p.title.clone(),
                origin: p.origin.clone(),
                status: match p.status {
                    ProposalStatus::Pending => "pending",
                    ProposalStatus::Invalid => "invalid",
                    ProposalStatus::Applied => "applied",
                    ProposalStatus::Rejected => "rejected",
                }
                .into(),
                stale: p.status == ProposalStatus::Pending && Some(p.base_revision) != revision,
                actions: p.actions.iter().map(|a| a.describe()).collect(),
                rationale: self.agent.rationale.get(&p.id).cloned().unwrap_or_default(),
                warnings: p.validation.warnings.clone(),
                notes: p.validation.notes.clone(),
                failed_action: p.validation.failed_action.as_ref().map(|(i, m)| {
                    format!(
                        "action {} ({}): {m}",
                        i + 1,
                        p.actions.get(*i).map(|a| a.describe()).unwrap_or_default()
                    )
                }),
                diff: p
                    .validation
                    .diff
                    .entries
                    .iter()
                    .map(|e| AgentDiffInfo {
                        change: e.change.into(),
                        domain: e.domain.into(),
                        kind: e.kind.clone(),
                        label: e.label.clone(),
                    })
                    .collect(),
                erc: delta(&p.validation.erc),
                drc: delta(&p.validation.drc),
                json: if self.agent.selected.as_deref() == Some(p.id.as_str()) {
                    serde_json::to_string_pretty(&p.to_json()).unwrap_or_default()
                } else {
                    String::new()
                },
            })
            .collect();
        AgentInfo {
            providers: self
                .agent
                .providers
                .iter()
                .map(|p| AgentProviderInfo {
                    id: p.id().into(),
                    name: p.name().into(),
                    description: p.description().into(),
                    kind: match p.kind() {
                        ProviderKind::RuleBased => "rule-based",
                        ProviderKind::External => "external input",
                        ProviderKind::Model => "model",
                    }
                    .into(),
                })
                .collect(),
            provider: self.agent.provider.clone(),
            proposals,
            selected: self.agent.selected.clone(),
            error: self.agent.error.clone(),
            audit: self
                .agent
                .api
                .audit
                .entries()
                .iter()
                .rev()
                .take(60)
                .map(|e| {
                    format!(
                        "#{} {} · {}{} · {}",
                        e.seq,
                        e.kind,
                        e.actor,
                        e.proposal
                            .as_ref()
                            .map(|p| format!(" · {p}"))
                            .unwrap_or_default(),
                        e.detail
                    )
                })
                .collect(),
        }
    }
}
