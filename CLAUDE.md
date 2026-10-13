# ButureCAD — CLAUDE.md

## 1. Project Identity

Project name: **ButureCAD**

ButureCAD is a professional, local-first Electronic Design Automation (EDA) application for:

- schematic capture;
- PCB design;
- interactive routing;
- ERC;
- DRC;
- component libraries;
- simulation;
- 3D PCB visualization;
- manufacturing output;
- programmable electronic design;
- plugins;
- optional AI-agent integration.

The long-term workflow is:

```text
Schematic
   ↓
ERC
   ↓
Footprints
   ↓
PCB
   ↓
Placement
   ↓
Routing
   ↓
DRC
   ↓
3D
   ↓
Manufacturing
```

ButureCAD must be a REAL engineering application.

Do not build a visual prototype pretending to be an EDA.

---

# 2. Core Philosophy

ButureCAD must be:

- local-first;
- offline-first;
- fast;
- lightweight;
- deterministic;
- modular;
- scriptable;
- extensible;
- reliable.

The user owns their projects completely.

The application must remain usable without an internet connection.

---

# 3. STRICTLY FORBIDDEN ARCHITECTURE

DO NOT introduce:

- Supabase;
- Firebase;
- mandatory PostgreSQL;
- cloud storage;
- mandatory APIs;
- mandatory login;
- authentication;
- user accounts;
- organizations;
- cloud workspaces;
- mandatory telemetry;
- mandatory remote services;
- Electron;
- server dependency for normal usage.

Do not add cloud architecture "for future scalability."

ButureCAD is intentionally LOCAL.

Plugins may optionally access external services when explicitly granted network permission.

---

# 4. Technology Stack

Desktop:

```text
Tauri 2
React
TypeScript
Vite
```

Core:

```text
Rust
```

Rendering:

```text
wgpu
```

Local persistence:

```text
Project files
SQLite only when justified for local cache/indexing
```

Testing should use the native testing ecosystem of each technology.

Do not replace these technologies without explicit instruction.

---

# 5. Fundamental Architecture

Target architecture:

```text
                  ButureCAD
                      │
          ┌───────────┴───────────┐
          │                       │
     Desktop GUI             buturecad-cli
          │                       │
          └───────────┬───────────┘
                      │
              ButureCAD Core
                    Rust
                      │
      ┌───────────────┼────────────────┐
      │               │                │
 Schematic           PCB             Rules
      │               │                │
      │            Router          ERC / DRC
      │               │                │
      └───────────────┼────────────────┘
                      │
                    EDA IR
                      │
        ┌─────────────┼─────────────┐
        │             │             │
 ButureScript      Plugins      Agent API
```

The Rust core is the source of truth.

---

# 6. Repository Structure

Prefer:

```text
apps/
  desktop/

crates/
  eda-core/
  geometry/
  renderer/
  schematic/
  pcb/
  router/
  rules/
  libraries/
  manufacturing/
  simulation/
  viewer3d/
  io/
  scripting/
  plugin-runtime/
  agent-api/

docs/
tests/
examples/
benchmarks/
```

Do not create unnecessary packages.

Keep dependencies directional and explicit.

Avoid circular dependencies.

---

# 7. GUI Is NOT The Core

This rule is extremely important.

React is responsible for:

- windows;
- panels;
- menus;
- dialogs;
- toolbar;
- inspector;
- command palette;
- UI state.

React must NOT become responsible for:

- PCB geometry;
- electrical connectivity;
- routing;
- DRC;
- ERC;
- geometry calculations;
- copper calculations;
- project serialization;
- manufacturing generation.

These belong in Rust.

---

# 8. Rendering

Complex PCB and schematic rendering must NOT depend on thousands of React DOM/SVG elements.

Primary rendering technology:

```text
wgpu
```

The renderer must eventually support very large designs.

Performance matters.

Avoid unnecessary allocations in rendering loops.

Avoid cloning large structures every frame.

Separate:

```text
Model
Renderer
Tools
UI
```

---

# 9. Units and Precision

EDA requires precision.

Never casually use floating-point coordinates throughout the PCB core.

Prefer integer internal coordinates using a sufficiently small base unit.

Examples could include nanometers or another carefully selected internal unit.

Conversions to:

- mm;
- mil;
- inch;

should happen at boundaries.

Document the chosen internal coordinate system.

Never silently lose precision.

---

# 10. Geometry Engine

Geometry is critical infrastructure.

Geometry algorithms must have tests.

Important operations include:

- intersection;
- distance;
- containment;
- nearest point;
- collision;
- polygon operations;
- offsets;
- arcs;
- bounding boxes;
- spatial queries.

Do not implement fragile geometry hacks just to make a demo work.

If an algorithm is incomplete, mark it clearly as incomplete.

---

# 11. Command Engine

All meaningful user mutations must eventually go through the Command Engine.

Examples:

```text
PlaceComponentCommand
MoveComponentCommand
DeleteComponentCommand
CreateWireCommand
CreateTrackCommand
CreateViaCommand
CreateZoneCommand
```

This enables:

- Undo;
- Redo;
- transactions;
- history;
- recovery;
- AI change review.

Do not bypass the Command Engine without a strong architectural reason.

---

# 12. Project Files

Projects must remain local.

Project format should be:

- deterministic;
- versionable;
- documented;
- recoverable;
- portable;
- Git-friendly when reasonable.

Saving must use safe/atomic strategies.

A crash during Save should not destroy the last valid project.

Maintain schema versions.

Provide migrations.

---

# 13. Schematic Requirements

The schematic architecture must eventually support:

- components;
- symbols;
- pins;
- wires;
- buses;
- junctions;
- labels;
- global labels;
- power symbols;
- hierarchical sheets;
- electrical connectivity;
- annotations.

Visual intersection does NOT necessarily mean electrical connection.

Connectivity must be represented logically.

---

# 14. PCB Requirements

The PCB architecture must eventually support:

- board outline;
- footprints;
- pads;
- tracks;
- vias;
- zones;
- keepouts;
- text;
- graphics;
- nets;
- multilayer boards.

PCB objects must be backed by real geometry.

A rendered track must correspond to an actual Track object.

Do not fake PCB functionality visually.

---

# 15. Routing

Interactive routing is a core ButureCAD feature.

Target capabilities:

```text
Basic Routing
↓
Walk Around
↓
Push & Shove
↓
Differential Pairs
↓
Length Tuning
```

Routing must consider:

- nets;
- clearances;
- pads;
- tracks;
- vias;
- zones;
- keepouts;
- board boundaries;
- design rules.

Do not claim Push & Shove exists if it only draws a preview.

---

# 16. DRC

Design Rule Checking must operate on actual PCB geometry.

Examples:

- clearance;
- track width;
- via diameter;
- via drill;
- copper-to-edge;
- shorts;
- unconnected nets;
- courtyard collisions.

Violations should reference actual offending objects.

DRC results must be reproducible.

---

# 17. ERC

Electrical Rules Check must operate on the connectivity graph.

Examples:

- incompatible pin types;
- unconnected required pins;
- duplicate references;
- power input without source;
- dangling wires.

ERC must not simply scan visual objects.

---

# 18. Manufacturing

Manufacturing output must represent the actual PCB model.

Target formats include:

- Gerber;
- Excellon;
- BOM;
- Pick and Place.

Do NOT create fake Gerber files for demonstration.

Gerber Viewer should parse generated manufacturing files independently when implemented.

---

# 19. Libraries

Library architecture must remain modular.

Concepts:

```text
Component
Symbol
Footprint
3D Model
Manufacturer Data
```

A Component can reference:

```text
Symbol
Footprint
3D Model
Manufacturer
MPN
Datasheet
Parameters
```

Library search should remain local and fast.

---

# 20. Plugin Architecture

Plugins are a first-class feature.

Target plugin categories:

```text
ComponentProvider
SymbolProvider
FootprintProvider
Model3DProvider
Importer
Exporter
RulePlugin
ToolPlugin
SimulationPlugin
```

Plugins should eventually run with restricted permissions.

Investigate WASM/WASI where appropriate.

Permissions may include:

```text
filesystem
network
project-read
project-write
```

Network access must never be assumed.

---

# 21. ButureScript

ButureCAD includes a code-first design language:

```text
ButureScript
```

Extension:

```text
.beda
```

Pipeline:

```text
ButureScript
     ↓
Lexer
     ↓
Parser
     ↓
AST
     ↓
Semantic Analysis
     ↓
EDA IR
     ↓
ButureCAD Core
```

ButureScript must NOT directly manipulate rendering.

The EDA IR is the bridge.

---

# 22. ButureScript Units

Units are part of the language.

Potential dimensions include:

```text
nm
um
mm
mil
inch

V
mV

A
mA

ohm
kOhm
MOhm

Hz
kHz
MHz
GHz
```

Dimensionally invalid operations must fail.

Example:

```text
10mm + 5V
```

must be rejected.

---

# 23. Visual + Code Design

ButureCAD should eventually support:

```text
VISUAL
CODE
SPLIT
```

ButureScript should generate the same internal model used by the visual editor.

Avoid creating separate incompatible implementations.

The architecture should support:

```text
Code
  ↓
EDA IR
  ↓
Core
  ↓
Visual representation
```

---

# 24. CLI

The project will eventually include:

```text
buturecad-cli
```

The CLI must reuse the SAME Rust Core.

Examples:

```bash
buturecad check project
buturecad erc project
buturecad drc project
buturecad export-gerber project
```

Do not reimplement core logic inside the CLI.

---

# 25. Headless Architecture

Core EDA functionality must eventually work without:

- React;
- Tauri;
- graphical window.

Target:

```text
Project
↓
EDA Core
↓
ERC
↓
DRC
↓
Manufacturing
```

This enables:

- automation;
- testing;
- scripting;
- CI;
- AI agents.

---

# 26. AI Philosophy

AI is OPTIONAL.

ButureCAD must remain completely functional without AI.

Never introduce an AI dependency into the EDA Core.

Agents are simply another client of the Core.

Architecture:

```text
Human GUI ───┐
             │
CLI ─────────┤
             ▼
        ButureCAD Core
             ▲
             │
ButureScript ┤
             │
AI Agent ────┘
```

---

# 27. Agent Safety

AI agents must NOT directly modify arbitrary project files.

Preferred flow:

```text
Agent
↓
Agent API
↓
Proposed Changes
↓
Validation
↓
Diff
↓
User Approval
↓
Command Engine
↓
Project
```

Agent actions should be auditable.

---

# 28. Never Fake Features

This rule is CRITICAL.

Do NOT implement:

- fake DRC;
- fake ERC;
- fake routing;
- fake simulation;
- fake Gerber;
- fake component search;
- fake AI output;
- buttons that only show success messages.

If a feature is incomplete:

mark it as incomplete.

Do not pretend it works.

---

# 29. No Placeholder Creep

Temporary placeholders are allowed only when required by the current implementation stage.

They must be clearly marked.

Do not let placeholders silently become permanent architecture.

Before major milestones search for:

```text
TODO
FIXME
mock
placeholder
temporary
fake
stub
```

---

# 30. Dependency Discipline

Before introducing a dependency ask:

1. Is it actually needed?
2. Can existing dependencies solve this?
3. Is it actively maintained?
4. Is its license compatible?
5. Is it excessively large?
6. Does it compromise offline usage?
7. Does it duplicate functionality?

Record significant dependencies in:

```text
docs/DEPENDENCIES.md
```

---

# 31. Commercial Licensing Awareness

ButureCAD may eventually be distributed commercially.

Do not casually copy source code from other EDA projects.

Especially avoid introducing copyleft source code directly into proprietary components without explicit review.

Using a file format or interoperating with another program is not the same as copying its implementation.

Document licensing decisions.

When uncertain, flag the issue rather than guessing.

---

# 32. Performance Philosophy

Do not optimize blindly.

Use:

```text
benchmark
↓
identify bottleneck
↓
optimize
↓
benchmark again
```

Important metrics:

- startup time;
- project load;
- RAM;
- frame time;
- routing latency;
- DRC time;
- ERC time;
- zone refill time.

Avoid unnecessary copies of PCB/project structures.

---

# 33. Testing

Important modules require tests.

Especially:

```text
geometry
project serialization
migrations
connectivity
ERC
DRC
routing
Gerber
ButureScript
```

Use:

- unit tests;
- integration tests;
- property-based tests;
- fuzzing where appropriate.

Tests should validate behavior, not implementation details.

---

# 34. Error Handling

Do not casually use:

```rust
unwrap()
expect()
panic!()
```

in production paths.

Return meaningful errors.

UI errors should be understandable.

Internal errors should preserve debugging context.

Malformed project files must not crash the entire application.

---

# 35. Rust Quality

Prefer:

- explicit ownership;
- small modules;
- clear domain types;
- meaningful enums;
- Result-based errors;
- minimal unsafe.

Avoid `unsafe` unless necessary.

Every unsafe block requires documentation explaining:

- why it is necessary;
- assumptions;
- invariants;
- why it is sound.

---

# 36. TypeScript Quality

Use TypeScript strictly.

Avoid:

```typescript
any
```

unless absolutely necessary.

Define typed interfaces between Tauri and frontend.

Do not duplicate Rust domain models manually when generated/shared contracts are practical.

---

# 37. UI Philosophy

ButureCAD is engineering software.

Optimize for:

- information density;
- precision;
- speed;
- keyboard usage;
- discoverability;
- low interaction cost.

Do NOT optimize primarily for flashy animations.

Animations should communicate state.

Do not make CAD interactions slower for visual effects.

---

# 38. Keyboard-First Workflow

Professional users should be able to work quickly.

Eventually support:

- shortcuts;
- command palette;
- repeat last command;
- quick tool switching;
- layer switching;
- contextual actions.

Avoid requiring excessive mouse travel.

---

# 39. Documentation

Maintain documentation as architecture evolves.

Important documents:

```text
docs/ARCHITECTURE.md
docs/ROADMAP.md
docs/DEPENDENCIES.md
docs/STATUS.md
docs/file-format/
docs/plugin-sdk/
docs/agent-api/
docs/buturescript/
```

Documentation must describe reality.

Do not document unfinished functionality as completed.

---

# 40. STATUS.md

Maintain:

```text
docs/STATUS.md
```

Use sections:

```text
# Working

# Partial

# Broken

# Planned

# Technical Debt

# Known Bugs
```

Update it after major milestones.

---

# 41. Before Every Major Feature

Before implementing a major feature:

1. inspect relevant existing architecture;
2. understand current abstractions;
3. identify reusable modules;
4. avoid duplicating functionality;
5. define integration points;
6. implement;
7. test;
8. compile;
9. inspect regressions.

Do not blindly create parallel systems.

---

# 42. After Every Major Feature

Run appropriate:

```bash
cargo fmt --check
cargo clippy
cargo test
```

and frontend equivalents.

Fix relevant failures introduced by the change.

Do not simply suppress warnings.

---

# 43. Checkpoint Protocol

After approximately 3–5 roadmap prompts, perform a checkpoint.

During checkpoint:

1. compile entire workspace;
2. execute tests;
3. inspect regressions;
4. search for TODO/FIXME/mock;
5. inspect architecture;
6. test recently implemented workflows;
7. update STATUS.md.

Do NOT add new functionality during a checkpoint.

---

# 44. Do Not Rewrite Working Systems

Before large refactors determine:

- what currently works;
- what depends on it;
- existing tests;
- compatibility requirements.

Do not delete functioning systems merely because another implementation looks cleaner.

Refactor incrementally.

---

# 45. Git Discipline

Keep changes logically grouped.

Do not mix unrelated refactors into feature implementation.

Commit messages should explain intent.

Recommended style:

```text
feat(pcb): add track routing model
feat(router): implement obstacle detection
fix(drc): handle via clearance correctly
refactor(geometry): unify distance calculations
test(router): add walk-around scenarios
docs(buturescript): document unit system
```

Never commit:

- generated build artifacts;
- secrets;
- API keys;
- temporary debug files.

---

# 46. External APIs

Core functionality must never depend on an external API.

Plugins MAY provide external integrations.

Examples:

```text
Component Supplier Plugin
Parts Search Plugin
Datasheet Plugin
AI Provider Plugin
```

If external API is unavailable:

ButureCAD must continue working.

---

# 47. Security

Treat project files and plugins as untrusted input.

Parsers must validate input.

Plugins should be sandboxed where practical.

Never execute arbitrary project code without explicit trust boundaries.

ButureScript should not automatically become arbitrary native code execution.

---

# 48. Manufacturing Safety

Manufacturing output is high consequence.

Before production export:

- validate board;
- run DRC;
- report warnings;
- report unresolved nets;
- validate board outline.

Never silently fix manufacturing errors.

Never claim fabrication readiness solely because export succeeded.

---

# 49. AI Must Not Hallucinate Engineering Success

If an AI agent generates a board:

that does NOT mean the board is electrically correct.

The system must still run:

```text
ERC
DRC
Design Rules
Manufacturing Validation
```

AI suggestions must be treated as proposed engineering changes.

---

# 50. Current Roadmap

The detailed implementation roadmap is stored separately.

When receiving a numbered roadmap prompt:

1. read this CLAUDE.md;
2. inspect current project state;
3. read STATUS.md;
4. implement only the requested stage;
5. preserve previous functionality;
6. run appropriate tests;
7. update documentation;
8. report what was ACTUALLY implemented.

Do not jump ahead unless required for architecture.

---

# 51. Definition of Done

A feature is DONE only when:

- implementation exists;
- it integrates with the actual Core;
- it is not a mock;
- expected interactions work;
- important errors are handled;
- relevant tests exist;
- project builds;
- documentation reflects reality.

A button existing in the UI does not mean the feature exists.

---

# 52. Final Engineering Principle

ButureCAD must evolve from:

```text
small
correct
tested
```

to:

```text
large
correct
tested
```

NOT from:

```text
large
fake
fragile
```

Prioritize engineering correctness over feature count.

The goal is not to LOOK like KiCad, Altium, or another EDA.

The goal is to become a genuinely useful electronic design system with its own architecture and identity.

---

# ButureCAD

Design visually.

Design with code.

Automate when useful.

Keep the engineer in control.