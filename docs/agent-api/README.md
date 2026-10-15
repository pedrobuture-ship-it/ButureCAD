# Agent API (roadmap 40) and AI Workspace (roadmap 41)

ButureCAD lets AI agents — and any external tool — **understand** a project and **propose**
changes. It never lets them change anything directly, and it contains no model and no network code:
AI is optional and is only ever another client of the Core.

```text
Agent ─▶ AgentSurface ─▶ Proposed Changes ─▶ Validation (on a COPY) ─▶ Diff + ERC/DRC
                                                     │
                       user ─ Apply / Reject ────────┘  (host-side Reviewer)
                                      └▶ Command Engine: ONE undoable transaction
```

Crate: `buturecad-agent-api` (depends on the Core, never the other way round).

## What an agent can call (`AgentSurface`)

* **READ** — `get_project get_schematic get_board get_components get_nets get_rules get_violations`.
  JSON views of the Core model (lengths in integer nm). `get_board` includes every pad position and
  net, so an agent can place parts near pins without guessing.
* **ANALYSIS** — `run_erc`, `run_drc`, `measure_net {net}` (routed length per layer, vias, pads,
  unrouted connections), `measure_track {id}`.
* **PROPOSE** — a list of **actions**:
  `create_component remove_component connect disconnect place move route create_via create_zone
  change_rule`. Each is a small JSON object with `"action": "<name>"` (schema: `crates/agent-api/src/action.rs`; the wire protocol document comes with stage 42). Components and nets are named the way engineers
  name them (references, net names), never by file paths.

There is **no method** on `AgentSurface` that changes a project. `apply` and `reject` belong to the
separate `Reviewer` trait, which only the host (the AI Workspace buttons, the CLI later) holds; the
stage-42 protocol exposes `AgentSurface` only.

## Proposals

`propose` never touches the real project. The actions run on a **clone** inside a scratch Command
Engine, producing:

* a **state diff** (`Diff`): schematic objects, footprints, nets, tracks, vias, zones, rules,
  library additions — computed by comparing before/after states, so it shows consequences the
  actions did not spell out (e.g. the footprint the board sync created);
* **ERC and DRC before → after** with the new and resolved findings;
* warnings (e.g. "ERC errors would increase"), per-action notes, and — if an action fails — the
  index and reason. A failing proposal is `invalid` and changes nothing.

**Apply** (host only) re-checks that the project is still at the validated revision (else
`stale`), replays the same actions inside one transaction (label `Agent: <title>`) and verifies
that the project equals the validated copy exactly; any difference rolls everything back. Undo
reverts the whole proposal in one step.

## Audit

Every call — reads and analyses included — becomes a structured `AuditEntry {seq, kind, actor,
proposal, detail}` (`read analyze propose validated invalid approved applied rejected
apply_failed`). The desktop app appends them as JSON Lines to `<project>/.buture/agent-audit.jsonl`.

## AI Workspace (stage 41)

Tab **AI Workspace** (`Ctrl+Shift+A`): choose a provider, write a request, press *Propose*. The
proposal shows its rationale, the actions, the diff (`+ − ~`), the ERC/DRC comparison, warnings and,
under *Inspect*, the raw structured proposal. *Apply* is enabled only for a valid, non-stale
proposal; *Reject* is final. The audit log is at the bottom.

### Providers (how requests become proposals)

`AgentProvider` (in `agent-api`) turns a request and a read-only project into drafts. The Core does
not know any model; connecting a local or remote one means implementing this trait outside the
Core (or speaking the stage-42 protocol) — no Core change. Shipped providers are **not AI**:

| Provider | Kind | What it does |
|---|---|---|
| Decoupling helper | rule-based | “Add decoupling to U1”: reads U1's power-input pins and their nets, finds the ground net, proposes one 100 nF per power pin and one 4.7 µF per supply net (`C14`, `C15`, … continuing after the highest used number), connects them and places each just outside its pad. |
| Proposal JSON | external input | Paste a proposal written by any model, script or person (`{title, rationale, actions}` or a bare action list). Parsed strictly. |

## Limits

* No language-model provider ships with the Core (by design). A model must be added as a provider
  or used through the protocol.
* `connect`/`disconnect` work with net labels and power symbols; parts joined by wires must be
  edited in the visual editor. `remove_component` also removes labels on its pins.
* Placement by the decoupling helper is geometric and simple: check the DRC comparison for
  courtyard overlaps before applying.
* Proposals are held in memory per session; the audit log is the persistent record.
