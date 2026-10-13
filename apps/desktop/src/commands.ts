/**
 * The single command registry. Menus, the toolbar, the command palette and keyboard shortcuts all
 * read from here, so a command cannot exist in one place and be missing in another.
 *
 * Only commands that do something real are enabled; others stay visible but disabled with the
 * reason, so nothing pretends to work.
 */
import type { Mode } from "./bindings/Mode";
import type { UiState } from "./store";

export type Category = "File" | "Edit" | "View" | "Mode" | "Tools" | "Help";

/** What a command can do. Implemented by `actions.ts`; faked in tests. */
export interface Actions {
  newProject(): void;
  openProject(): void;
  save(): void;
  saveAs(): void;
  closeProject(): void;
  renameProject(): void;
  undo(): void;
  redo(): void;
  selectAll(): void;
  cancel(): void;
  setMode(mode: Mode): void;
  toggleTheme(): void;
  toggleUnits(): void;
  toggleGrid(): void;
  toggleSnapGrid(): void;
  toggleSnapObjects(): void;
  cycleGrid(): void;
  zoomFit(): void;
  zoomIn(): void;
  zoomOut(): void;
  toggleBottomPanel(): void;
  toggleInspector(): void;
  toggleDebugOverlay(): void;
  openPalette(): void;
  showAbout(): void;
}

export interface AppCommand {
  id: string;
  title: string;
  category: Category;
  shortcut?: string;
  /** `true` when runnable. A string is the reason it is unavailable. */
  available?: (s: UiState) => true | string;
  run: (a: Actions) => void;
}

const hasProject = (s: UiState): true | string =>
  s.snapshot?.project ? true : "Open or create a project first";

const hasCanvas = (s: UiState): true | string => {
  if (!s.snapshot?.project) return "Open or create a project first";
  const m = s.snapshot.mode;
  return m === "schematic" || m === "pcb" ? true : "This mode has no canvas yet";
};

export function buildCommands(): AppCommand[] {
  const modeCommand = (mode: Mode, title: string, shortcut: string): AppCommand => ({
    id: `mode.${mode}`,
    title: `Switch to ${title}`,
    category: "Mode",
    shortcut,
    available: () => true,
    run: (a) => a.setMode(mode),
  });
  return [
    {
      id: "file.new",
      title: "New Project…",
      category: "File",
      shortcut: "Ctrl+N",
      run: (a) => a.newProject(),
    },
    {
      id: "file.open",
      title: "Open Project…",
      category: "File",
      shortcut: "Ctrl+O",
      run: (a) => a.openProject(),
    },
    {
      id: "file.save",
      title: "Save",
      category: "File",
      shortcut: "Ctrl+S",
      available: (s) => (s.snapshot?.project ? true : "No project is open"),
      run: (a) => a.save(),
    },
    {
      id: "file.saveAs",
      title: "Save As…",
      category: "File",
      shortcut: "Ctrl+Shift+S",
      available: hasProject,
      run: (a) => a.saveAs(),
    },
    {
      id: "file.rename",
      title: "Rename Project…",
      category: "File",
      available: hasProject,
      run: (a) => a.renameProject(),
    },
    {
      id: "file.close",
      title: "Close Project",
      category: "File",
      shortcut: "Ctrl+W",
      available: hasProject,
      run: (a) => a.closeProject(),
    },

    {
      id: "edit.undo",
      title: "Undo",
      category: "Edit",
      shortcut: "Ctrl+Z",
      available: (s) => (s.snapshot?.project?.can_undo ? true : "Nothing to undo"),
      run: (a) => a.undo(),
    },
    {
      id: "edit.redo",
      title: "Redo",
      category: "Edit",
      shortcut: "Ctrl+Y",
      available: (s) => (s.snapshot?.project?.can_redo ? true : "Nothing to redo"),
      run: (a) => a.redo(),
    },
    {
      id: "edit.selectAll",
      title: "Select All",
      category: "Edit",
      shortcut: "Ctrl+A",
      available: hasCanvas,
      run: (a) => a.selectAll(),
    },
    {
      id: "edit.cancel",
      title: "Clear Selection / Cancel",
      category: "Edit",
      shortcut: "Escape",
      available: hasCanvas,
      run: (a) => a.cancel(),
    },

    {
      id: "view.fit",
      title: "Zoom to Fit",
      category: "View",
      shortcut: "Home",
      available: hasCanvas,
      run: (a) => a.zoomFit(),
    },
    {
      id: "view.zoomIn",
      title: "Zoom In",
      category: "View",
      shortcut: "Ctrl++",
      available: hasCanvas,
      run: (a) => a.zoomIn(),
    },
    {
      id: "view.zoomOut",
      title: "Zoom Out",
      category: "View",
      shortcut: "Ctrl+-",
      available: hasCanvas,
      run: (a) => a.zoomOut(),
    },
    {
      id: "view.grid",
      title: "Toggle Grid",
      category: "View",
      shortcut: "Ctrl+'",
      available: hasCanvas,
      run: (a) => a.toggleGrid(),
    },
    {
      id: "view.cycleGrid",
      title: "Cycle Grid Size",
      category: "View",
      shortcut: "Alt+G",
      available: hasCanvas,
      run: (a) => a.cycleGrid(),
    },
    {
      id: "view.snapGrid",
      title: "Toggle Grid Snap",
      category: "View",
      available: hasCanvas,
      run: (a) => a.toggleSnapGrid(),
    },
    {
      id: "view.snapObjects",
      title: "Toggle Object Snap",
      category: "View",
      available: hasCanvas,
      run: (a) => a.toggleSnapObjects(),
    },
    {
      id: "view.units",
      title: "Toggle Units (mm / mil)",
      category: "View",
      shortcut: "Ctrl+Shift+U",
      run: (a) => a.toggleUnits(),
    },
    {
      id: "view.theme",
      title: "Toggle Dark / Light Theme",
      category: "View",
      shortcut: "Ctrl+Shift+L",
      run: (a) => a.toggleTheme(),
    },
    {
      id: "view.bottom",
      title: "Toggle Bottom Panel",
      category: "View",
      shortcut: "Ctrl+J",
      run: (a) => a.toggleBottomPanel(),
    },
    {
      id: "view.inspector",
      title: "Toggle Inspector",
      category: "View",
      shortcut: "Ctrl+I",
      run: (a) => a.toggleInspector(),
    },
    {
      id: "view.debug",
      title: "Toggle FPS / Debug Overlay",
      category: "View",
      shortcut: "F3",
      available: hasCanvas,
      run: (a) => a.toggleDebugOverlay(),
    },

    modeCommand("schematic", "Schematic", "Ctrl+1"),
    modeCommand("pcb", "PCB", "Ctrl+2"),
    modeCommand("symbol_editor", "Symbol Editor", "Ctrl+3"),
    modeCommand("footprint_editor", "Footprint Editor", "Ctrl+4"),
    modeCommand("viewer3d", "3D Viewer", "Ctrl+5"),

    {
      id: "tools.palette",
      title: "Command Palette…",
      category: "Tools",
      shortcut: "Ctrl+Shift+P",
      run: (a) => a.openPalette(),
    },
    { id: "help.about", title: "About ButureCAD", category: "Help", run: (a) => a.showAbout() },
  ];
}

export function availability(cmd: AppCommand, state: UiState): true | string {
  return cmd.available ? cmd.available(state) : true;
}

/** Subsequence match score (higher is better); `null` when `query` does not match. */
export function fuzzyScore(query: string, text: string): number | null {
  const q = query.toLowerCase().replace(/\s+/g, "");
  const t = text.toLowerCase();
  if (q === "") return 0;
  let ti = 0;
  let score = 0;
  let streak = 0;
  for (const ch of q) {
    const found = t.indexOf(ch, ti);
    if (found === -1) return null;
    streak = found === ti ? streak + 1 : 0;
    score += 1 + streak * 2 + (found === 0 || t[found - 1] === " " ? 3 : 0);
    ti = found + 1;
  }
  return score - (t.length - q.length) * 0.01;
}

/** Commands matching `query`, best first; ties keep registry order. */
export function filterCommands(commands: AppCommand[], query: string): AppCommand[] {
  return commands
    .map((c, i) => ({ c, i, s: fuzzyScore(query, `${c.category} ${c.title}`) }))
    .filter((x): x is { c: AppCommand; i: number; s: number } => x.s !== null)
    .sort((a, b) => b.s - a.s || a.i - b.i)
    .map((x) => x.c);
}
