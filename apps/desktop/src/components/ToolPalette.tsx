import * as Icons from "lucide-react";
import type { LucideIcon } from "lucide-react";
import { useUi } from "../store";
import { TOOLS } from "../tools";
import { useActions } from "./hooks";

function icon(name: string): LucideIcon {
  const found = (Icons as unknown as Record<string, LucideIcon | undefined>)[name];
  return found ?? Icons.Square;
}

export function ToolPalette() {
  const snap = useUi((s) => s.snapshot);
  const actions = useActions();
  const tools = (snap && TOOLS[snap.mode]) ?? [];
  return (
    <div className="toolpalette" role="toolbar" aria-label="Tools" aria-orientation="vertical">
      {tools.map((t) => {
        const Icon = icon(t.icon);
        const active = snap?.tool === t.id;
        const usable = t.implemented && !!snap?.project;
        const tip = `${t.label}${t.shortcut ? ` (${t.shortcut})` : ""}${
          t.implemented ? "" : ` — not implemented yet, planned in ${t.planned ?? "a later stage"}`
        }${t.implemented && !snap?.project ? " — open a project first" : ""}`;
        return (
          <button
            key={t.id}
            className={active ? "tp-btn active" : "tp-btn"}
            disabled={!usable}
            title={tip}
            aria-label={t.label}
            aria-pressed={active}
            onClick={() => actions.selectTool(t.id)}
          >
            <Icon size={18} />
          </button>
        );
      })}
    </div>
  );
}
