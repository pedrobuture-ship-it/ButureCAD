import * as Icons from "lucide-react";
import type { LucideIcon } from "lucide-react";
import { TOOLS } from "../tools";
import { useUi } from "../store";

function icon(name: string): LucideIcon {
  const found = (Icons as unknown as Record<string, LucideIcon | undefined>)[name];
  return found ?? Icons.Square;
}

export function ToolPalette() {
  const snap = useUi((s) => s.snapshot);
  const tools = (snap && TOOLS[snap.mode]) ?? [];
  return (
    <div className="toolpalette" role="toolbar" aria-label="Tools" aria-orientation="vertical">
      {tools.map((t) => {
        const Icon = icon(t.icon);
        const active = snap?.tool === t.id;
        const tip = `${t.label}${t.shortcut ? ` (${t.shortcut})` : ""}${t.implemented ? "" : ` — not implemented yet, planned in ${t.planned ?? "a later stage"}`}`;
        return (
          <button
            key={t.id}
            className={active ? "tp-btn active" : "tp-btn"}
            disabled={!t.implemented}
            title={tip}
            aria-label={t.label}
            aria-pressed={active}
          >
            <Icon size={18} />
          </button>
        );
      })}
    </div>
  );
}
