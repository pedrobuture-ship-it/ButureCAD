import { useEffect, useRef, useState } from "react";
import { availability, runCommand, type Category } from "../commands";
import { formatShortcut } from "../shortcuts";
import { useUi } from "../store";
import { useActions, useCommands } from "./hooks";

const MENUS: { label: string; category: Category }[] = [
  { label: "File", category: "File" },
  { label: "Edit", category: "Edit" },
  { label: "View", category: "View" },
  { label: "Mode", category: "Mode" },
  { label: "Tools", category: "Tools" },
  { label: "Help", category: "Help" },
];

/** Menus are generated from the command registry; items are never hand-wired. */
export function MenuBar() {
  const commands = useCommands();
  const actions = useActions();
  const state = useUi((s) => s);
  const [open, setOpen] = useState<Category | null>(null);
  const ref = useRef<HTMLDivElement>(null);

  useEffect(() => {
    if (!open) return;
    const close = (e: MouseEvent) => {
      if (!ref.current?.contains(e.target as Node)) setOpen(null);
    };
    window.addEventListener("mousedown", close);
    return () => window.removeEventListener("mousedown", close);
  }, [open]);

  return (
    <div className="menubar" ref={ref} role="menubar">
      <span className="brand">ButureCAD</span>
      {MENUS.map((m) => (
        <div key={m.category} className="menu-root">
          <button
            role="menuitem"
            className={open === m.category ? "menu-btn open" : "menu-btn"}
            onClick={() => setOpen(open === m.category ? null : m.category)}
            onMouseEnter={() => open && setOpen(m.category)}
          >
            {m.label}
          </button>
          {open === m.category && (
            <div className="menu-pop" role="menu">
              {commands
                .filter((c) => c.category === m.category)
                .map((c) => {
                  const a = availability(c, state);
                  return (
                    <button
                      key={c.id}
                      role="menuitem"
                      className="menu-item"
                      disabled={a !== true}
                      title={a === true ? undefined : a}
                      onClick={() => {
                        setOpen(null);
                        runCommand(c, actions);
                      }}
                    >
                      <span>{c.title}</span>
                      {c.shortcut && <kbd>{formatShortcut(c.shortcut)}</kbd>}
                    </button>
                  );
                })}
            </div>
          )}
        </div>
      ))}
    </div>
  );
}
