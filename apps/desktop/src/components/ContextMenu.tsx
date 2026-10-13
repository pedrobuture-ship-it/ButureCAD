import { useEffect } from "react";
import { availability } from "../commands";
import { formatShortcut } from "../shortcuts";
import { store, useUi } from "../store";
import { useActions, useCommands } from "./hooks";

/** Right-click menu for the canvas. Items come from the registry, so only real commands appear. */
const ITEMS = [
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
];

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
        top: Math.min(menu.y, window.innerHeight - 330),
      }}
      role="menu"
      onMouseDown={(e) => e.stopPropagation()}
    >
      {sel > 0 && (
        <div className="ctx-head">
          {sel === 1 ? (state.snapshot?.selection[0]?.title ?? "") : `${sel} objects`}
        </div>
      )}
      {ITEMS.map((id, i) => {
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
              c.run(actions);
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
