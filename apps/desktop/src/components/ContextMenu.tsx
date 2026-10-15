import { useEffect } from "react";
import { availability, runCommand } from "../commands";
import { formatShortcut } from "../shortcuts";
import { store, useUi } from "../store";
import { useActions, useCommands } from "./hooks";

/** Right-click menu for the canvas. Items come from the registry, so only real commands appear. */
const COMMON_TAIL = [
  "-",
  "edit.selectAll",
  "edit.cancel",
  "-",
  "view.fit",
  "view.zoomIn",
  "view.zoomOut",
  "-",
  "view.grid",
  "view.snapObjects",
  "-",
  "edit.undo",
  "edit.redo",
  "edit.repeat",
];

const SCHEMATIC_ITEMS = [
  "edit.rotate",
  "edit.mirrorX",
  "edit.mirrorY",
  "edit.duplicate",
  "edit.delete",
  "-",
  "edit.copy",
  "edit.cut",
  "edit.paste",
  ...COMMON_TAIL,
];

/** The PCB has Flip instead of mirroring, locking, alignment, and no clipboard yet. */
const PCB_ITEMS = [
  "edit.rotate",
  "pcb.flip",
  "pcb.lock",
  "pcb.unlock",
  "edit.duplicate",
  "edit.delete",
  "-",
  "pcb.alignLeft",
  "pcb.alignRight",
  "pcb.alignTop",
  "pcb.alignBottom",
  "pcb.alignCenterH",
  "pcb.alignCenterV",
  "pcb.distributeH",
  "pcb.distributeV",
  "-",
  "pcb.fillZones",
  "pcb.layerNext",
  ...COMMON_TAIL,
];

const EDITOR_ITEMS = ["edit.rotate", "edit.mirrorX", "edit.mirrorY", "edit.delete", ...COMMON_TAIL];

function itemsFor(mode: string | undefined): string[] {
  if (mode === "pcb") return PCB_ITEMS;
  if (mode === "symbol_editor" || mode === "footprint_editor") return EDITOR_ITEMS;
  return SCHEMATIC_ITEMS;
}

export function ContextMenu() {
  const menu = useUi((s) => s.contextMenu);
  const state = useUi((s) => s);
  const commands = useCommands();
  const actions = useActions();

  useEffect(() => {
    if (!menu) return;
    const close = () => store.set({ contextMenu: null });
    const onKey = (e: KeyboardEvent) => e.key === "Escape" && close();
    window.addEventListener("mousedown", close);
    window.addEventListener("keydown", onKey);
    window.addEventListener("blur", close);
    return () => {
      window.removeEventListener("mousedown", close);
      window.removeEventListener("keydown", onKey);
      window.removeEventListener("blur", close);
    };
  }, [menu]);

  if (!menu) return null;
  const sel = state.snapshot?.selection.length ?? 0;
  return (
    <div
      className="ctx"
      style={{
        left: Math.min(menu.x, window.innerWidth - 230),
        top: Math.max(4, Math.min(menu.y, window.innerHeight - 560)),
      }}
      role="menu"
      onMouseDown={(e) => e.stopPropagation()}
    >
      {sel > 0 && (
        <div className="ctx-head">
          {sel === 1 ? (state.snapshot?.selection[0]?.title ?? "") : `${sel} objects`}
        </div>
      )}
      {itemsFor(state.snapshot?.mode).map((id, i) => {
        if (id === "-") return <div key={`s${i}`} className="ctx-sep" />;
        const c = commands.find((x) => x.id === id);
        if (!c) return null;
        const a = availability(c, state);
        return (
          <button
            key={id}
            role="menuitem"
            className="menu-item"
            disabled={a !== true}
            title={a === true ? undefined : a}
            onClick={() => {
              store.set({ contextMenu: null });
              runCommand(c, actions);
            }}
          >
            <span>{c.title}</span>
            {c.shortcut && <kbd>{formatShortcut(c.shortcut)}</kbd>}
          </button>
        );
      })}
    </div>
  );
}
