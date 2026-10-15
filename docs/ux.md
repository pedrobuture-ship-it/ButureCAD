# UX audit and keyboard workflow (roadmap stage 46)

One command registry (`apps/desktop/src/commands.ts`) feeds the menus, the toolbar, the context
menu, the command palette and the keyboard. Every entry point runs commands through
`runCommand`, which also records them in the command history (`history.ts`, ids only, kept in the
viewer's local storage; nothing leaves the machine). Unavailable commands stay visible with the
reason.

## Added in this stage

| Feature | How | Notes |
|---|---|---|
| **Repeat last command** | `Ctrl+Enter`, Edit menu, palette | Repeats the most recent *repeatable* command that is available now (editing commands and tools; not file/view/mode/help/undo/cancel). With nothing selected, a delete falls through to the previous usable command rather than failing. |
| **Recent tools** | `Q` = Previous Tool; palette lists recently used commands first (tagged "Recent") when the query is empty and gives them a small tie-break when searching | `Q` alternates between the last two tools usable in the current view. |
| **Command palette** | `Ctrl+Shift+P` (already existed) | Now recent-first. |
| **Copper layer switching** | `PageDown` / `PageUp` in the PCB view | Cycles F.Cu → inner layers → B.Cu using the stack order from the core. |
| **Shortcut cheat sheet** | `F1`, Help menu, palette | Generated from the registry, so it cannot drift from the real bindings. |
| **Context menu per view** | right click | Schematic, PCB (flip, lock, align/distribute, fill zones, layer), symbol/footprint editors; includes Repeat. |
| **Conflict check** | `history.test.ts` | For each view (schematic, PCB, symbol editor, footprint editor), with and without a selection, no two available commands share a key. |

## Audit result

| Area | State |
|---|---|
| Keyboard shortcuts | ✔ Registry-driven, conflict-tested per view. Letter keys are tools/edits; `Ctrl` combinations are file/clipboard/mode. |
| Context menus | ✔ Per view. Only commands that work are enabled. |
| Selection / multi-selection | ✔ Click, `Shift`/`Ctrl`+click to add or toggle, box select, `Ctrl+A`. |
| Drag / move | ✔ Drag, `G` (drag with wire stretch), `M` (carry the selection with the pointer, schematic). |
| Copy / cut / paste | ✔ Schematic. ✘ **PCB** (Duplicate works; copy/cut/paste of board objects do not exist — shown as unavailable with the reason). |
| Duplicate | ✔ Schematic and PCB. |
| Align / distribute | ✔ **PCB** (footprints, tracks, vias; locked objects are skipped). ✘ **Schematic**: not implemented — moving parts must also stretch their wires without conflicting edits, which needs its own design. |
| Properties | ✔ Inspector for each view. |
| Tool switching | ✔ Keys, toolbar, palette, `Q`. |
| Layer switching | ✔ Inspector (click) and now `PageUp/PageDown`. No direct "go to layer N" keys. |
| Routing controls | ✔ `X` route, `Shift+X` pair, `V` via/layer change, `Space` flip elbow, `Enter` finish, `Esc` cancel, `T` tune. |
| Zoom / pan | ✔ Wheel zoom at the cursor, `Home` fit, `Ctrl++/-`, middle-drag or `Space`+drag pan. ✘ No keyboard panning (arrow keys), no "zoom to selection". |
| Nudge selection with arrow keys | ✘ Missing. |

## Limits

* Shortcuts are fixed; there is no keymap editor.
* The history is per machine/profile and is not synchronised.
* Interactive behaviour (pointer handling, canvas) is covered by Rust workspace tests and the
  registry by unit tests; there is no automated end-to-end test that drives the real window.
