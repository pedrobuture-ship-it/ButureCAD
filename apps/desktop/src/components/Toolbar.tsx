import {
  FolderOpen,
  FilePlus2,
  Save,
  Undo2,
  Redo2,
  Maximize,
  ZoomIn,
  ZoomOut,
  Grid3x3,
  Magnet,
  Sun,
  Moon,
  Gauge,
  Terminal,
} from "lucide-react";
import type { ReactNode } from "react";
import { availability, runCommand } from "../commands";
import { formatShortcut } from "../shortcuts";
import { useUi } from "../store";
import { useActions, useCommands } from "./hooks";

function Sep() {
  return <span className="tb-sep" />;
}

export function Toolbar() {
  const commands = useCommands();
  const actions = useActions();
  const state = useUi((s) => s);
  const snap = state.snapshot;

  const btn = (id: string, icon: ReactNode, active?: boolean) => {
    const c = commands.find((x) => x.id === id);
    if (!c) return null;
    const a = availability(c, state);
    const tip = `${c.title}${c.shortcut ? ` (${formatShortcut(c.shortcut)})` : ""}${a === true ? "" : ` — ${a}`}`;
    return (
      <button
        key={id}
        className={active ? "tb-btn active" : "tb-btn"}
        disabled={a !== true}
        title={tip}
        aria-label={c.title}
        onClick={() => runCommand(c, actions)}
      >
        {icon}
      </button>
    );
  };

  return (
    <div className="toolbar" role="toolbar" aria-label="Main toolbar">
      {btn("file.new", <FilePlus2 size={16} />)}
      {btn("file.open", <FolderOpen size={16} />)}
      {btn("file.save", <Save size={16} />)}
      <Sep />
      {btn("edit.undo", <Undo2 size={16} />)}
      {btn("edit.redo", <Redo2 size={16} />)}
      <Sep />
      {btn("view.fit", <Maximize size={16} />)}
      {btn("view.zoomIn", <ZoomIn size={16} />)}
      {btn("view.zoomOut", <ZoomOut size={16} />)}
      <Sep />
      {btn("view.grid", <Grid3x3 size={16} />, snap?.grid.visible)}
      {btn("view.snapObjects", <Magnet size={16} />, snap?.grid.snap_objects)}
      <Sep />
      {btn("view.theme", snap?.theme === "light" ? <Moon size={16} /> : <Sun size={16} />)}
      {btn("view.debug", <Gauge size={16} />, snap?.debug_overlay)}
      <span className="tb-spacer" />
      {btn("tools.palette", <Terminal size={16} />)}
    </div>
  );
}
