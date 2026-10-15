//! The agent-facing surface, proposals, validation and the host-side reviewer.

use crate::action::Action;
use crate::analysis::{self, Finding};
use crate::apply::perform_all;
use crate::audit::AuditLog;
use crate::diff::{diff, Diff};
use crate::{read, FootprintResolver};
use buturecad_eda_core::{CommandEngine, Project};
use serde::Serialize;
use serde_json::{json, Value};
use std::collections::{BTreeMap, BTreeSet};

#[derive(Debug, thiserror::Error)]
pub enum ApiError {
    #[error("unknown query “{0}”")]
    UnknownQuery(String),
    #[error("{0}")]
    Failed(String),
    #[error("no proposal “{0}”")]
    NoProposal(String),
    #[error("proposal {0} is {1} and cannot be applied")]
    NotApplicable(String, &'static str),
    #[error(
        "the project changed after proposal {0} was validated (revision {1} → {2}); propose again"
    )]
    Stale(String, u64, u64),
    #[error(
        "applying proposal {0} did not reproduce the validated result, so nothing was kept: {1}"
    )]
    Mismatch(String, String),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ProposalStatus {
    Pending,
    Invalid,
    Applied,
    Rejected,
}

impl ProposalStatus {
    fn word(self) -> &'static str {
        match self {
            ProposalStatus::Pending => "pending",
            ProposalStatus::Invalid => "invalid",
            ProposalStatus::Applied => "already applied",
            ProposalStatus::Rejected => "rejected",
        }
    }
}

#[derive(Debug, Clone, Serialize)]
pub struct FindingJson {
    pub severity: String,
    pub rule: String,
    pub message: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct Delta {
    pub errors_before: usize,
    pub errors_after: usize,
    pub warnings_before: usize,
    pub warnings_after: usize,
    /// Findings that exist after the change but not before.
    pub new: Vec<FindingJson>,
    /// Findings that existed before and are gone.
    pub resolved: usize,
}

#[derive(Debug, Clone, Serialize)]
pub struct Validation {
    pub ok: bool,
    /// Index and message of the action that failed, if any.
    pub failed_action: Option<(usize, String)>,
    pub notes: Vec<String>,
    pub warnings: Vec<String>,
    pub diff: Diff,
    pub erc: Option<Delta>,
    pub drc: Option<Delta>,
}

#[derive(Debug, Clone)]
pub struct Proposal {
    pub id: String,
    pub title: String,
    pub origin: String,
    pub base_revision: u64,
    pub actions: Vec<Action>,
    pub status: ProposalStatus,
    pub validation: Validation,
    /// The project exactly as it will be after applying (kept to verify the real apply).
    expected: Option<Project>,
}

impl Proposal {
    pub fn to_json(&self) -> Value {
        json!({
            "id": self.id, "title": self.title, "origin": self.origin, "base_revision": self.base_revision,
            "status": self.status, "actions": self.actions, "validation": self.validation,
        })
    }
}

fn delta(before: &[Finding], after: &[Finding]) -> Delta {
    let count = |f: &[Finding], s: &str| f.iter().filter(|x| x.severity == s).count();
    let key = |f: &Finding| format!("{}|{}", f.rule, f.message);
    let b: BTreeSet<String> = before.iter().map(key).collect();
    let a: BTreeSet<String> = after.iter().map(key).collect();
    Delta {
        errors_before: count(before, "error"),
        errors_after: count(after, "error"),
        warnings_before: count(before, "warning"),
        warnings_after: count(after, "warning"),
        new: after
            .iter()
            .filter(|f| !b.contains(&key(f)))
            .map(|f| FindingJson {
                severity: f.severity.clone(),
                rule: f.rule.clone(),
                message: f.message.clone(),
            })
            .collect(),
        resolved: b.difference(&a).count(),
    }
}

/// Everything an agent may call. There is deliberately no method here that changes a project.
pub trait AgentSurface {
    /// `get_project`, `get_schematic`, `get_board`, `get_components`, `get_nets`, `get_rules`,
    /// `get_violations`.
    fn read(
        &mut self,
        project: &Project,
        revision: u64,
        what: &str,
        actor: &str,
    ) -> Result<Value, ApiError>;
    /// `run_erc`, `run_drc`, `measure_net` (`{"net": …}`), `measure_track` (`{"id": …}`).
    fn analyze(
        &mut self,
        project: &Project,
        what: &str,
        args: &Value,
        actor: &str,
    ) -> Result<Value, ApiError>;
    /// Validates the actions on a copy and records the proposal. Never changes the project.
    fn propose(
        &mut self,
        project: &Project,
        revision: u64,
        title: &str,
        actions: Vec<Action>,
        actor: &str,
    ) -> Proposal;
    fn proposal(&self, id: &str) -> Option<&Proposal>;
}

/// Host-side decisions. Not reachable through the agent protocol.
pub trait Reviewer {
    fn apply(
        &mut self,
        engine: &mut CommandEngine,
        id: &str,
        approver: &str,
    ) -> Result<(), ApiError>;
    fn reject(&mut self, id: &str, approver: &str, reason: &str) -> Result<(), ApiError>;
}

pub struct AgentApi {
    resolve: FootprintResolver,
    proposals: BTreeMap<String, Proposal>,
    order: Vec<String>,
    next: u64,
    pub audit: AuditLog,
}

impl AgentApi {
    pub fn new(resolve: FootprintResolver) -> Self {
        Self {
            resolve,
            proposals: BTreeMap::new(),
            order: Vec::new(),
            next: 1,
            audit: AuditLog::default(),
        }
    }

    /// All proposals, oldest first.
    pub fn proposals(&self) -> Vec<&Proposal> {
        self.order
            .iter()
            .filter_map(|i| self.proposals.get(i))
            .collect()
    }

    fn validate(&self, project: &Project, actions: &[Action]) -> (Validation, Option<Project>) {
        let mut scratch = CommandEngine::new(project.clone());
        let res = scratch.transaction("validate", |e| {
            perform_all(e, actions, &self.resolve).map_err(|(i, m)| {
                buturecad_eda_core::CommandError::from(
                    buturecad_eda_core::model::ModelError::BadObject(format!("action {i}: {m}")),
                )
            })
        });
        // `perform_all` errors carry the index; re-run to get them structured when it failed.
        match res {
            Ok(notes) => {
                let after = scratch.into_project();
                let (e0, e1) = (
                    analysis::erc_findings(project),
                    analysis::erc_findings(&after),
                );
                let (d0, d1) = (
                    analysis::drc_findings(project),
                    analysis::drc_findings(&after),
                );
                let (erc, drc) = (delta(&e0, &e1), delta(&d0, &d1));
                let mut warnings = Vec::new();
                if erc.errors_after > erc.errors_before {
                    warnings.push(format!(
                        "ERC errors would increase from {} to {}",
                        erc.errors_before, erc.errors_after
                    ));
                }
                if drc.errors_after > drc.errors_before {
                    warnings.push(format!(
                        "DRC errors would increase from {} to {}",
                        drc.errors_before, drc.errors_after
                    ));
                }
                if diff(project, &after).is_empty() {
                    warnings.push("the proposal changes nothing".into());
                }
                let v = Validation {
                    ok: true,
                    failed_action: None,
                    notes,
                    warnings,
                    diff: diff(project, &after),
                    erc: Some(erc),
                    drc: Some(drc),
                };
                (v, Some(after))
            }
            Err(_) => {
                let mut again = CommandEngine::new(project.clone());
                let fail = perform_all(&mut again, actions, &self.resolve).err();
                (
                    Validation {
                        ok: false,
                        failed_action: fail.clone(),
                        notes: Vec::new(),
                        warnings: Vec::new(),
                        diff: Diff::default(),
                        erc: None,
                        drc: None,
                    },
                    None,
                )
            }
        }
    }
}

impl AgentSurface for AgentApi {
    fn read(
        &mut self,
        project: &Project,
        revision: u64,
        what: &str,
        actor: &str,
    ) -> Result<Value, ApiError> {
        let v = match what {
            "get_project" => read::get_project(project, revision),
            "get_schematic" => read::get_schematic(project),
            "get_board" => read::get_board(project),
            "get_components" => read::get_components(project),
            "get_nets" => read::get_nets(project),
            "get_rules" => read::get_rules(project),
            "get_violations" => analysis::get_violations(project),
            other => return Err(ApiError::UnknownQuery(other.into())),
        };
        self.audit
            .record("read", actor, None, json!({"query": what}));
        Ok(v)
    }

    fn analyze(
        &mut self,
        project: &Project,
        what: &str,
        args: &Value,
        actor: &str,
    ) -> Result<Value, ApiError> {
        let v = match what {
            "run_erc" => analysis::run_erc(project),
            "run_drc" => analysis::run_drc(project),
            "measure_net" => {
                let n = args
                    .get("net")
                    .and_then(Value::as_str)
                    .ok_or_else(|| ApiError::Failed("measure_net needs {\"net\": name}".into()))?;
                analysis::measure_net(project, n).map_err(ApiError::Failed)?
            }
            "measure_track" => {
                let id = args.get("id").and_then(Value::as_u64).ok_or_else(|| {
                    ApiError::Failed("measure_track needs {\"id\": number}".into())
                })?;
                analysis::measure_track(project, id).map_err(ApiError::Failed)?
            }
            other => return Err(ApiError::UnknownQuery(other.into())),
        };
        self.audit.record(
            "analyze",
            actor,
            None,
            json!({"analysis": what, "args": args}),
        );
        Ok(v)
    }

    fn propose(
        &mut self,
        project: &Project,
        revision: u64,
        title: &str,
        actions: Vec<Action>,
        actor: &str,
    ) -> Proposal {
        let id = format!("P{}", self.next);
        self.next += 1;
        self.audit.record(
            "propose",
            actor,
            Some(&id),
            json!({"title": title, "actions": actions}),
        );
        let (validation, expected) = self.validate(project, &actions);
        let status = if validation.ok {
            ProposalStatus::Pending
        } else {
            ProposalStatus::Invalid
        };
        self.audit.record(
            if validation.ok { "validated" } else { "invalid" },
            "buturecad",
            Some(&id),
            json!({"diff_entries": validation.diff.entries.len(), "failed_action": validation.failed_action, "warnings": validation.warnings}),
        );
        let p = Proposal {
            id: id.clone(),
            title: title.to_owned(),
            origin: actor.to_owned(),
            base_revision: revision,
            actions,
            status,
            validation,
            expected,
        };
        self.order.push(id.clone());
        self.proposals.insert(id, p.clone());
        p
    }

    fn proposal(&self, id: &str) -> Option<&Proposal> {
        self.proposals.get(id)
    }
}

impl Reviewer for AgentApi {
    fn apply(
        &mut self,
        engine: &mut CommandEngine,
        id: &str,
        approver: &str,
    ) -> Result<(), ApiError> {
        let p = self
            .proposals
            .get(id)
            .ok_or_else(|| ApiError::NoProposal(id.into()))?;
        if p.status != ProposalStatus::Pending {
            return Err(ApiError::NotApplicable(id.into(), p.status.word()));
        }
        if engine.revision() != p.base_revision {
            return Err(ApiError::Stale(
                id.into(),
                p.base_revision,
                engine.revision(),
            ));
        }
        let expected = p
            .expected
            .clone()
            .ok_or_else(|| ApiError::Failed("the proposal has no validated result".into()))?;
        let actions = p.actions.clone();
        let title = p.title.clone();
        self.audit.record("approved", approver, Some(id), json!({}));
        let resolve = &self.resolve;
        let res = engine.transaction(format!("Agent: {title}"), |e| {
            perform_all(e, &actions, resolve).map_err(|(i, m)| {
                buturecad_eda_core::CommandError::from(
                    buturecad_eda_core::model::ModelError::BadObject(format!("action {i}: {m}")),
                )
            })?;
            if *e.project() != expected {
                return Err(buturecad_eda_core::CommandError::from(
                    buturecad_eda_core::model::ModelError::BadObject(
                        "the result differs from the validated copy".into(),
                    ),
                ));
            }
            Ok(())
        });
        match res {
            Ok(()) => {
                if let Some(p) = self.proposals.get_mut(id) {
                    p.status = ProposalStatus::Applied;
                }
                self.audit.record(
                    "applied",
                    approver,
                    Some(id),
                    json!({"undo_label": format!("Agent: {title}")}),
                );
                Ok(())
            }
            Err(e) => {
                self.audit.record(
                    "apply_failed",
                    approver,
                    Some(id),
                    json!({"error": e.to_string()}),
                );
                Err(ApiError::Mismatch(id.into(), e.to_string()))
            }
        }
    }

    fn reject(&mut self, id: &str, approver: &str, reason: &str) -> Result<(), ApiError> {
        let p = self
            .proposals
            .get_mut(id)
            .ok_or_else(|| ApiError::NoProposal(id.into()))?;
        if p.status == ProposalStatus::Applied {
            return Err(ApiError::NotApplicable(id.into(), p.status.word()));
        }
        p.status = ProposalStatus::Rejected;
        self.audit
            .record("rejected", approver, Some(id), json!({"reason": reason}));
        Ok(())
    }
}
