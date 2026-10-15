# Code ↔ Visual (roadmap stage 37)

## What works today: Code → Visual

Tab **Code** (`Ctrl+0`) with three views:

* **CODE** — the editor (line numbers, error lines marked, diagnostics list with `line:col`; click a
  diagnostic to jump to it). `Ctrl+S` saves `design.beda` next to the project.
* **SPLIT** — editor on the left, live preview on the right.
* **VISUAL** — the preview alone.

On every change (debounced 350 ms) the core runs `parse → validate → EDA IR → preview`. The
preview is a private second `Workspace` whose project is generated from the script in a temporary
folder by exactly the same core code that builds any project (`buturecad_scripting::compile` +
`buturecad_eda_core::ir_apply`). So the schematic, PCB and 3D previews use the real renderers and
show what the core builds, not a re-interpretation. The preview is **read-only** (the wheel zooms,
dragging pans; nothing else reaches it) and keeps its zoom/pan across rebuilds. While the script
has errors the last good preview stays and the diagnostics say what is wrong.

*Generate project…* creates a real project folder from the script (through the Command Engine),
saves `design.beda` in it and opens it in the normal visual editors. After that the project and the
script are two separate things: editing visually does not change the script (see below).

## Design for Visual → Code (not implemented)

Goal: visual edits must never destroy hand-written code. The plan:

1. **Text is the source of truth.** The AST already carries a span for every statement and
   expression; formatting and comments are never regenerated.
2. **Provenance.** Elaboration records, for every IR object, the AST node(s) that produced it.
   A `component R1 {…}` or `place R1 at (10mm, 5mm)` written literally has one origin; objects
   produced by a `for` loop, a module instance or a computed expression have a derived origin.
3. **Edits become text patches** (replace a literal range, insert a statement, delete a statement),
   computed from spans and applied to the text with a revision check, so a stale visual edit is
   rejected instead of overwriting newer code:
   * moving a part whose `place` has literal coordinates → patch the literal;
   * moving a part whose position comes from an expression/loop → the object is shown as
     *generated*; the editor offers to add an explicit override statement after the generator
     instead of rewriting the loop;
   * new objects drawn visually → appended inside a clearly marked managed block at the end of the
     file (`// @visual … // @end`), which the user may move or edit freely;
   * deleting an object with a single literal origin → remove that statement; derived origins are
     refused with an explanation.
4. **Round trip test:** `compile(patch(text, edit)) == apply(edit, compile(text))` for every
   supported edit, with comments and formatting of untouched code byte-identical.

Nothing of step 2–4 exists yet; this stage delivers the architecture hooks (spans, deterministic
elaboration, the IR boundary) and Code → Visual.
