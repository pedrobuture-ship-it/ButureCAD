//! `buturecad agent …`: serve the agent protocol on stdio and let the human review proposals.
//!
//! * `serve` — JSON-RPC on stdin/stdout (see `docs/agent-api/protocol.md`). The agent can inspect,
//!   analyse and propose; proposals are validated on a copy and stored under
//!   `<project>/.buture/proposals/`. Nothing here applies anything.
//! * `proposals`, `show`, `apply --yes`, `reject` — the human's side. `apply` re-validates against
//!   the current project, refuses stale proposals, applies one undoable transaction and saves.

use buturecad_agent_api::{
    fingerprint, serve, AgentApi, AgentSurface, Proposal, ProposalStatus, Reviewer, Server,
};
use buturecad_eda_core::{CommandEngine, Project};
use buturecad_io::{session::STATE_DIR, ProjectSession};
use buturecad_libraries::LibraryDir;
use serde_json::{json, Value};
use std::io::{BufRead, Write};
use std::path::{Path, PathBuf};

fn dir(project: &str) -> PathBuf {
    Path::new(project).join(STATE_DIR).join("proposals")
}

fn read_file(path: &Path) -> Result<Value, String> {
    serde_json::from_slice(
        &std::fs::read(path).map_err(|e| format!("cannot read {}: {e}", path.display()))?,
    )
    .map_err(|e| format!("{}: {e}", path.display()))
}

fn stored(project: &str) -> Vec<Value> {
    let mut v: Vec<(u64, Value)> = std::fs::read_dir(dir(project))
        .into_iter()
        .flatten()
        .flatten()
        .filter_map(|e| {
            let n = e
                .file_name()
                .to_string_lossy()
                .strip_prefix('P')?
                .strip_suffix(".json")?
                .parse::<u64>()
                .ok()?;
            Some((n, read_file(&e.path()).ok()?))
        })
        .collect();
    v.sort_by_key(|x| x.0);
    v.into_iter().map(|x| x.1).collect()
}

fn next_id(project: &str) -> u64 {
    stored(project)
        .iter()
        .filter_map(|p| p["id"].as_str()?.strip_prefix('P')?.parse::<u64>().ok())
        .max()
        .unwrap_or(0)
        + 1
}

fn write_proposal(project: &str, fp: &str, p: &Proposal) -> Result<(), String> {
    let d = dir(project);
    std::fs::create_dir_all(&d).map_err(|e| e.to_string())?;
    let mut v = p.to_json();
    v["base_fingerprint"] = json!(fp);
    let mut text = serde_json::to_string_pretty(&v).map_err(|e| e.to_string())?;
    text.push('\n');
    buturecad_io::atomic::atomic_write(&d.join(format!("{}.json", p.id)), text.as_bytes())
        .map_err(|e| e.to_string())
}

fn append_audit(project: &str, lines: &str) {
    let d = Path::new(project).join(STATE_DIR);
    let _ = std::fs::create_dir_all(&d);
    if let Ok(mut f) = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(d.join("agent-audit.jsonl"))
    {
        let _ = f.write_all(lines.as_bytes());
    }
}

pub fn libs_for(project: &str, extra: Option<&str>) -> Vec<(String, LibraryDir)> {
    let mut v = vec![(
        "project".to_owned(),
        LibraryDir::new(Path::new(project).join("libraries")),
    )];
    if let Some(e) = extra {
        v.push(("user".to_owned(), LibraryDir::new(e)));
    }
    v
}

/// Runs the protocol server. Only protocol messages go to `out`.
pub fn cmd_serve(
    project: &str,
    libs: Option<&str>,
    input: &mut dyn BufRead,
    out: &mut dyn Write,
) -> Result<i32, String> {
    let opened = ProjectSession::open(project)
        .map_err(|e| format!("cannot open the project “{project}”: {e}"))?;
    let p = opened.project;
    let fp = fingerprint(&p);
    let mut server = Server::new(p, Box::new(buturecad_scripting::resolve_footprint))
        .with_libraries(libs_for(project, libs));
    // Keep proposal numbers unique across sessions.
    *server.api_mut() = AgentApi::new(Box::new(buturecad_scripting::resolve_footprint))
        .with_next_id(next_id(project));
    let (mut audited, mut written): (usize, Vec<(String, ProposalStatus)>) = (0, Vec::new());
    let mut io_err = None;
    let r = serve(&mut server, input, out, |s| {
        let entries = s.api().audit.entries();
        if entries.len() > audited {
            let text: String = entries[audited..]
                .iter()
                .filter_map(|e| serde_json::to_string(e).ok())
                .map(|l| l + "\n")
                .collect();
            append_audit(project, &text);
            audited = entries.len();
        }
        for p in s.api().proposals() {
            if !written.iter().any(|(i, st)| *i == p.id && *st == p.status) {
                if let Err(e) = write_proposal(project, &fp, p) {
                    io_err = Some(e);
                }
                written.retain(|(i, _)| *i != p.id);
                written.push((p.id.clone(), p.status));
            }
        }
    });
    r.map_err(|e| e.to_string())?;
    match io_err {
        Some(e) => Err(format!("cannot store a proposal: {e}")),
        None => Ok(0),
    }
}

pub fn cmd_list(
    project: &str,
    json_out: bool,
    quiet: bool,
    out: &mut dyn Write,
) -> Result<i32, String> {
    let list = stored(project);
    if json_out {
        let _ = writeln!(out, "{}", serde_json::to_string_pretty(&json!({"proposals": list.iter().map(|p| json!({"id": p["id"], "title": p["title"], "origin": p["origin"], "status": p["status"]})).collect::<Vec<_>>()})).unwrap_or_default());
    } else if !quiet {
        if list.is_empty() {
            let _ = writeln!(out, "no proposals");
        }
        for p in &list {
            let _ = writeln!(
                out,
                "{:<6} {:<9} {:<28} from {}",
                p["id"].as_str().unwrap_or("?"),
                p["status"].as_str().unwrap_or("?"),
                p["title"].as_str().unwrap_or(""),
                p["origin"].as_str().unwrap_or("")
            );
        }
    }
    Ok(0)
}

fn load_stored(project: &str, id: &str) -> Result<Value, String> {
    if !id.starts_with('P') || id.len() > 12 || !id[1..].chars().all(|c| c.is_ascii_digit()) {
        return Err(format!("“{id}” is not a proposal id (P1, P2, …)"));
    }
    read_file(&dir(project).join(format!("{id}.json")))
        .map_err(|_| format!("no stored proposal {id}; see `buturecad agent proposals`"))
}

fn revalidate(
    project: &str,
    id: &str,
) -> Result<(Value, Project, CommandEngine, AgentApi, Proposal), String> {
    let stored = load_stored(project, id)?;
    let opened = ProjectSession::open(project)
        .map_err(|e| format!("cannot open the project “{project}”: {e}"))?;
    let current = opened.project;
    let fp = fingerprint(&current);
    if stored["base_fingerprint"].as_str() != Some(&fp) {
        return Err(format!("proposal {id} was made for a different state of the project (stale); ask the agent to propose again"));
    }
    let actions: Vec<buturecad_agent_api::Action> =
        serde_json::from_value(stored["actions"].clone())
            .map_err(|e| format!("stored actions are invalid: {e}"))?;
    let title = stored["title"].as_str().unwrap_or("Proposal").to_owned();
    let origin = stored["origin"].as_str().unwrap_or("agent").to_owned();
    let engine = CommandEngine::new(current.clone());
    let mut api = AgentApi::new(Box::new(buturecad_scripting::resolve_footprint));
    let p = api.propose(&current, engine.revision(), &title, actions, &origin);
    Ok((stored, current, engine, api, p))
}

pub fn cmd_show(
    project: &str,
    id: &str,
    json_out: bool,
    quiet: bool,
    out: &mut dyn Write,
) -> Result<i32, String> {
    let (stored, _, _, _, p) = revalidate(project, id)?;
    if json_out {
        let _ = writeln!(
            out,
            "{}",
            serde_json::to_string_pretty(&p.to_json()).unwrap_or_default()
        );
    } else if !quiet {
        let _ = writeln!(
            out,
            "{} · {} (from {}, stored status {})",
            id,
            p.title,
            p.origin,
            stored["status"].as_str().unwrap_or("?")
        );
        for a in &p.actions {
            let _ = writeln!(out, "  action: {}", a.describe());
        }
        if let Some((i, m)) = &p.validation.failed_action {
            let _ = writeln!(out, "INVALID — action {}: {m}", i + 1);
        }
        for l in p.validation.diff.lines() {
            let _ = writeln!(out, "  {l}");
        }
        for (name, d) in [("ERC", &p.validation.erc), ("DRC", &p.validation.drc)] {
            if let Some(d) = d {
                let _ = writeln!(
                    out,
                    "  {name}: errors {} → {}, warnings {} → {}",
                    d.errors_before, d.errors_after, d.warnings_before, d.warnings_after
                );
                for f in &d.new {
                    let _ = writeln!(out, "    new {}[{}]: {}", f.severity, f.rule, f.message);
                }
            }
        }
        for w in &p.validation.warnings {
            let _ = writeln!(out, "  warning: {w}");
        }
    }
    Ok(i32::from(!p.validation.ok))
}

fn set_status(project: &str, id: &str, status: &str) -> Result<(), String> {
    let path = dir(project).join(format!("{id}.json"));
    let mut v = read_file(&path)?;
    v["status"] = json!(status);
    let mut text = serde_json::to_string_pretty(&v).map_err(|e| e.to_string())?;
    text.push('\n');
    buturecad_io::atomic::atomic_write(&path, text.as_bytes()).map_err(|e| e.to_string())
}

pub fn cmd_apply(project: &str, id: &str, yes: bool, out: &mut dyn Write) -> Result<i32, String> {
    if !yes {
        return Err(
            "applying changes a project; review it with `agent show` and pass --yes to confirm"
                .into(),
        );
    }
    let stored_status = load_stored(project, id)?["status"]
        .as_str()
        .unwrap_or("")
        .to_owned();
    if stored_status != "pending" {
        return Err(format!(
            "proposal {id} is {stored_status} and cannot be applied"
        ));
    }
    let (_, _, mut engine, mut api, p) = revalidate(project, id)?;
    if !p.validation.ok {
        return Err(format!("proposal {id} is invalid; nothing was changed"));
    }
    api.apply(&mut engine, &p.id, "cli-user")
        .map_err(|e| e.to_string())?;
    let session = ProjectSession::open(project)
        .map_err(|e| e.to_string())?
        .session;
    session
        .save(engine.project())
        .map_err(|e| format!("cannot save the project: {e}"))?;
    set_status(project, id, "applied")?;
    let mut lines = String::new();
    for e in api.audit.entries() {
        let mut e = e.clone();
        e.detail["stored_id"] = json!(id);
        lines += &(serde_json::to_string(&e).unwrap_or_default() + "\n");
    }
    append_audit(project, &lines);
    let _ = writeln!(
        out,
        "applied {id} and saved the project ({} change(s))",
        p.validation.diff.entries.len()
    );
    Ok(0)
}

pub fn cmd_reject(
    project: &str,
    id: &str,
    reason: &str,
    out: &mut dyn Write,
) -> Result<i32, String> {
    let st = load_stored(project, id)?["status"]
        .as_str()
        .unwrap_or("")
        .to_owned();
    if st == "applied" {
        return Err(format!("proposal {id} was already applied"));
    }
    set_status(project, id, "rejected")?;
    append_audit(project, &(json!({"kind": "rejected", "actor": "cli-user", "proposal": id, "detail": {"reason": reason}}).to_string() + "\n"));
    let _ = writeln!(out, "rejected {id}");
    Ok(0)
}
