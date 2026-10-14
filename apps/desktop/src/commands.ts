/**
 * The single command registry. Menus, the toolbar, the command palette and keyboard shortcuts all
 * read from here, so a command cannot exist in one place and be missing in another.
 *
 * Only commands that do something real are enabled; others stay visible but disabled with the
 * reason, so nothing pretends to work.
 */
import type { Mode } from "./bindings/Mode";
import type { UiState } from "./store";

export type Category =
  "File" | "Edit" | "View" | "Mode" | "Tools" | "Symbol" | "Footprint" | "Help";

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
  /** Activates a tool; tools that need an argument open their picker/prompt first. */
  selectTool(id: string): void;
  /** Runs a named Rust editor command (rotate, delete, paste, …). */
  editor(name: string): void;
  symbolNew(): void;
  symbolOpen(): void;
  symbolSaveToLibrary(): void;
  symbolCommand(name: string, arg?: string): void;
  runErc(): void;
  runDrc(): void;
  pcbPlaceFootprint(): void;
  pcbUpdateFromSchematic(): void;
  netClasses(): void;
  layerStack(): void;
  pcbCommand(name: string, arg?: string): void;
  footprintNew(): void;
  footprintOpen(): void;
  footprintGenerate(): void;
  footprintSaveToLibrary(): void;
  footprintCommand(name: string, arg?: string): void;
  startMove(): void;
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

const inSchematic = (s: UiState): true | string => {
  if (!s.snapshot?.project) return "Open or create a project first";
  return s.snapshot.mode === "schematic" ? true : "Switch to the Schematic view";
};

const inSymbolEditor = (s: UiState): true | string => {
  if (!s.snapshot?.project) return "Open or create a project first";
  if (s.snapshot.mode !== "symbol_editor") return "Switch to the Symbol Editor";
  return s.snapshot.symbol_editor ? true : "Open or create a symbol first";
};

const inFootprintEditor = (s: UiState): true | string => {
  if (!s.snapshot?.project) return "Open or create a project first";
  if (s.snapshot.mode !== "footprint_editor") return "Switch to the Footprint Editor";
  return s.snapshot.footprint_editor ? true : "Open, create or generate a footprint first";
};

const inPcb = (s: UiState): true | string => {
  if (!s.snapshot?.project) return "Open or create a project first";
  return s.snapshot.mode === "pcb" ? true : "Switch to the PCB view";
};

const hasPcbSelection = (s: UiState): true | string => {
  const a = inPcb(s);
  if (a !== true) return a;
  return (s.snapshot?.selection.length ?? 0) > 0 ? true : "Select something first";
};

/** Editing commands that work in the schematic, the PCB and the symbol/footprint editors. */
const inEditor = (s: UiState): true | string => {
  if (s.snapshot?.mode === "pcb") return inPcb(s);
  if (s.snapshot?.mode === "symbol_editor") return inSymbolEditor(s);
  if (s.snapshot?.mode === "footprint_editor") return inFootprintEditor(s);
  return inSchematic(s);
};

const hasSelection = (s: UiState): true | string => {
  const a = inEditor(s);
  if (a !== true) return a;
  return (s.snapshot?.selection.length ?? 0) > 0 ? true : "Select something first";
};

/** Duplicate works in the schematic and the PCB. */
const canDuplicate = (s: UiState): true | string =>
  s.snapshot?.mode === "pcb" ? hasPcbSelection(s) : hasSchematicSelection(s);

/** Like `hasSelection`, for commands the Symbol Editor does not implement. */
const hasSchematicSelection = (s: UiState): true | string =>
  s.snapshot?.mode === "symbol_editor" || s.snapshot?.mode === "footprint_editor"
    ? "Not available in the symbol and footprint editors"
    : hasSelection(s);

const hasCanvas = (s: UiState): true | string => {
  if (!s.snapshot?.project) return "Open or create a project first";
  const m = s.snapshot.mode;
  if (m === "symbol_editor")
    return s.snapshot.symbol_editor ? true : "Open or create a symbol first";
  if (m === "footprint_editor")
    return s.snapshot.footprint_editor ? true : "Open, create or generate a footprint first";
  return m === "schematic" || m === "pcb" ? true : "This mode has no canvas yet";
};

function symbolToolCommands(): AppCommand[] {
  const t = (id: string, title: string, shortcut: string): AppCommand => ({
    id: `symtool.${id}`,
    title: `Symbol Editor: ${title}`,
    category: "Tools",
    shortcut,
    available: inSymbolEditor,
    run: (a) => a.selectTool(id),
  });
  return [
    t("pin", "Place Pin", "P"),
    t("line", "Draw Line", "L"),
    t("rect", "Draw Rectangle", "B"),
    t("circle", "Draw Circle", "C"),
    t("arc", "Draw Arc", "A"),
    t("text", "Place Text…", "T"),
  ];
}

function footprintToolCommands(): AppCommand[] {
  const t = (id: string, title: string, shortcut: string): AppCommand => ({
    id: `fptool.${id}`,
    title: `Footprint Editor: ${title}`,
    category: "Tools",
    shortcut,
    available: inFootprintEditor,
    run: (a) => a.selectTool(id),
  });
  return [
    t("pad_smd", "Place SMD Pad", "S"),
    t("pad_through", "Place Through-hole Pad", "H"),
    t("npth", "Place Non-plated Hole", "N"),
    t("line", "Draw Line", "L"),
    t("rect", "Draw Rectangle", "B"),
    t("circle", "Draw Circle", "C"),
    t("arc", "Draw Arc", "A"),
    t("text", "Place Text…", "T"),
  ];
}

function pcbCommands(): AppCommand[] {
  const tool = (id: string, title: string, shortcut?: string): AppCommand => ({
    id: `pcbtool.${id}`,
    title: `${title} (PCB)`,
    category: "Tools",
    shortcut,
    available: inPcb,
    run: (a) => a.selectTool(id),
  });
  const edit = (id: string, title: string, name: string, shortcut?: string): AppCommand => ({
    id: `pcb.${id}`,
    title: `${title} (PCB)`,
    category: "Edit",
    shortcut,
    available: hasPcbSelection,
    run: (a) => a.editor(name),
  });
  return [
    {
      id: "pcb.update",
      title: "Update PCB from Schematic…",
      category: "Tools",
      shortcut: "F8",
      available: (s) => (s.snapshot?.project ? true : "Open or create a project first"),
      run: (a) => a.pcbUpdateFromSchematic(),
    },
    {
      id: "pcb.layer_stack",
      title: "Layer Stack Manager…",
      category: "Tools",
      available: (s) => (s.snapshot?.project ? true : "Open or create a project first"),
      run: (a) => a.layerStack(),
    },
    {
      id: "pcb.net_classes",
      title: "Net Classes and Design Rules…",
      category: "Tools",
      available: (s) => (s.snapshot?.project ? true : "Open or create a project first"),
      run: (a) => a.netClasses(),
    },
    {
      id: "pcb.ratsnest",
      title: "Show / Hide Ratsnest",
      category: "View",
      shortcut: "Shift+R",
      available: inPcb,
      run: (a) => a.pcbCommand("toggle_ratsnest"),
    },
    tool("route", "Route Track", "X"),
    tool("route_pair", "Route Differential Pair", "Shift+X"),
    tool("tune", "Tune Length", "T"),
    tool("zone", "Copper Zone", "Z"),
    {
      id: "pcb.fillZones",
      title: "Fill All Zones (PCB)",
      category: "Edit",
      shortcut: "B",
      available: (s) => (s.snapshot?.pcb?.zones ? true : "There are no zones on the board"),
      run: (a) => a.pcbCommand("fill_zones"),
    },
    {
      id: "pcb.unfillZones",
      title: "Clear All Zone Fills (PCB)",
      category: "Edit",
      shortcut: "Ctrl+B",
      available: (s) => (s.snapshot?.pcb?.zones ? true : "There are no zones on the board"),
      run: (a) => a.pcbCommand("unfill_zones"),
    },
    {
      id: "pcb.tuneSelected",
      title: "Tune Length of the Selected Track (PCB)",
      category: "Edit",
      available: hasPcbSelection,
      run: (a) => a.editor("tune_selected"),
    },
    tool("via", "Place Via", "Shift+V"),
    {
      id: "pcb.routeVia",
      title: "Via and Change Layer While Routing (PCB)",
      category: "Edit",
      shortcut: "V",
      available: (s) => (s.snapshot?.pcb?.route ? true : "No route in progress"),
      run: (a) => a.editor("route_via"),
    },
    {
      id: "pcb.finishRoute",
      title: "Finish Route or Zone Outline (PCB)",
      category: "Edit",
      shortcut: "Enter",
      available: (s) =>
        s.snapshot?.pcb?.route || s.snapshot?.pcb?.zone_drawing
          ? true
          : "No route or zone in progress",
      run: (a) => a.editor("route_finish"),
    },
    tool("place_footprint", "Place Footprint…", "A"),
    tool("outline_rect", "Draw Board Outline (rectangle)", "O"),
    tool("outline_line", "Draw Board Outline (polyline)"),
    edit("flip", "Flip to the Other Side", "flip", "F"),
    edit("lock", "Lock Footprint", "lock", "K"),
    edit("unlock", "Unlock Footprint", "unlock", "Shift+K"),
    edit("alignLeft", "Align Left", "align_left"),
    edit("alignRight", "Align Right", "align_right"),
    edit("alignTop", "Align Top", "align_top"),
    edit("alignBottom", "Align Bottom", "align_bottom"),
    edit("alignCenterH", "Align Centres Horizontally", "align_center_h"),
    edit("alignCenterV", "Align Centres Vertically", "align_center_v"),
    edit("distributeH", "Distribute Horizontally", "distribute_h"),
    edit("distributeV", "Distribute Vertically", "distribute_v"),
  ];
}

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
      id: "tool.select",
      title: "Select Tool",
      category: "Tools",
      shortcut: "V",
      available: (s) => (s.snapshot?.pcb?.route ? "V places a via while routing" : hasCanvas(s)),
      run: (a) => a.selectTool("select"),
    },
    {
      id: "tool.place_component",
      title: "Place Component…",
      category: "Tools",
      shortcut: "A",
      available: inSchematic,
      run: (a) => a.selectTool("place_component"),
    },
    {
      id: "tool.wire",
      title: "Draw Wire",
      category: "Tools",
      shortcut: "W",
      available: inSchematic,
      run: (a) => a.selectTool("wire"),
    },
    {
      id: "tool.bus",
      title: "Draw Bus",
      category: "Tools",
      shortcut: "B",
      available: inSchematic,
      run: (a) => a.selectTool("bus"),
    },
    {
      id: "tool.junction",
      title: "Place Junction",
      category: "Tools",
      shortcut: "J",
      available: inSchematic,
      run: (a) => a.selectTool("junction"),
    },
    {
      id: "tool.net_label",
      title: "Place Net Label…",
      category: "Tools",
      shortcut: "L",
      available: inSchematic,
      run: (a) => a.selectTool("net_label"),
    },
    {
      id: "tool.global_label",
      title: "Place Global Label…",
      category: "Tools",
      shortcut: "Ctrl+L",
      available: inSchematic,
      run: (a) => a.selectTool("global_label"),
    },
    {
      id: "tool.power",
      title: "Place Power Symbol…",
      category: "Tools",
      shortcut: "P",
      available: inSchematic,
      run: (a) => a.selectTool("power"),
    },
    {
      id: "tool.move",
      title: "Move Selection",
      category: "Tools",
      shortcut: "M",
      available: inSchematic,
      run: (a) => a.startMove(),
    },
    {
      id: "tool.drag",
      title: "Drag Tool",
      category: "Tools",
      shortcut: "G",
      available: inSchematic,
      run: (a) => a.selectTool("drag"),
    },

    {
      id: "edit.rotate",
      title: "Rotate",
      category: "Edit",
      shortcut: "R",
      available: inEditor,
      run: (a) => a.editor("rotate"),
    },
    {
      id: "edit.mirrorX",
      title: "Mirror Left-Right",
      category: "Edit",
      shortcut: "X",
      available: (s) => (s.snapshot?.mode === "pcb" ? "Use Flip in the PCB editor" : inEditor(s)),
      run: (a) => a.editor("mirror_x"),
    },
    {
      id: "edit.mirrorY",
      title: "Mirror Top-Bottom",
      category: "Edit",
      shortcut: "Y",
      available: (s) => (s.snapshot?.mode === "pcb" ? "Use Flip in the PCB editor" : inEditor(s)),
      run: (a) => a.editor("mirror_y"),
    },
    {
      id: "edit.delete",
      title: "Delete",
      category: "Edit",
      shortcut: "Delete",
      available: hasSelection,
      run: (a) => a.editor("delete"),
    },
    {
      id: "edit.duplicate",
      title: "Duplicate",
      category: "Edit",
      shortcut: "Ctrl+D",
      available: canDuplicate,
      run: (a) => a.editor("duplicate"),
    },
    {
      id: "edit.copy",
      title: "Copy",
      category: "Edit",
      shortcut: "Ctrl+C",
      available: hasSchematicSelection,
      run: (a) => a.editor("copy"),
    },
    {
      id: "edit.cut",
      title: "Cut",
      category: "Edit",
      shortcut: "Ctrl+X",
      available: hasSchematicSelection,
      run: (a) => a.editor("cut"),
    },
    {
      id: "edit.paste",
      title: "Paste",
      category: "Edit",
      shortcut: "Ctrl+V",
      available: (s) => {
        const a = inSchematic(s);
        if (a !== true) return a;
        return s.snapshot?.can_paste ? true : "The clipboard is empty";
      },
      run: (a) => a.editor("paste"),
    },
    {
      id: "edit.unitNext",
      title: "Next Unit of the Part Being Placed",
      category: "Edit",
      shortcut: "U",
      available: (s) =>
        s.snapshot?.mode === "schematic" && s.snapshot.tool === "place_component"
          ? true
          : "Arm the Place Component tool with a multi-unit symbol first",
      run: (a) => a.editor("unit_next"),
    },
    {
      id: "edit.flipElbow",
      title: "Flip Wire / Track Elbow",
      category: "Edit",
      shortcut: "Space",
      available: (s) => (s.snapshot?.mode === "pcb" ? inPcb(s) : inSchematic(s)),
      run: (a) => a.editor("flip_elbow"),
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

    ...symbolToolCommands(),
    ...footprintToolCommands(),
    ...pcbCommands(),
    {
      id: "footprint.new",
      title: "New Footprint…",
      category: "Footprint",
      available: hasProject,
      run: (a) => a.footprintNew(),
    },
    {
      id: "footprint.open",
      title: "Open Footprint…",
      category: "Footprint",
      available: hasProject,
      run: (a) => a.footprintOpen(),
    },
    {
      id: "footprint.generate",
      title: "Generate Footprint (DIP, SOIC, QFN, BGA, header)…",
      category: "Footprint",
      available: hasProject,
      run: (a) => a.footprintGenerate(),
    },
    {
      id: "footprint.save",
      title: "Save Footprint to Library…",
      category: "Footprint",
      available: inFootprintEditor,
      run: (a) => a.footprintSaveToLibrary(),
    },
    {
      id: "footprint.courtyard",
      title: "Footprint: Courtyard from Pads",
      category: "Footprint",
      available: inFootprintEditor,
      run: (a) => a.footprintCommand("auto_courtyard"),
    },
    {
      id: "tools.erc",
      title: "Run Electrical Rules Check",
      category: "Tools",
      shortcut: "Ctrl+Shift+E",
      available: hasProject,
      run: (a) => a.runErc(),
    },
    {
      id: "tools.drc",
      title: "Run Design Rules Check",
      category: "Tools",
      shortcut: "Ctrl+Shift+D",
      available: hasProject,
      run: (a) => a.runDrc(),
    },
    {
      id: "symbol.new",
      title: "New Symbol…",
      category: "Symbol",
      available: hasProject,
      run: (a) => a.symbolNew(),
    },
    {
      id: "symbol.open",
      title: "Open Symbol…",
      category: "Symbol",
      available: hasProject,
      run: (a) => a.symbolOpen(),
    },
    {
      id: "symbol.save",
      title: "Save Symbol to Library…",
      category: "Symbol",
      available: inSymbolEditor,
      run: (a) => a.symbolSaveToLibrary(),
    },
    {
      id: "symbol.addUnit",
      title: "Add Unit to Symbol",
      category: "Symbol",
      available: inSymbolEditor,
      run: (a) => a.symbolCommand("add_unit"),
    },
    {
      id: "symbol.createAlternate",
      title: "Create Alternate Representation",
      category: "Symbol",
      available: (s) => {
        const a = inSymbolEditor(s);
        if (a !== true) return a;
        return s.snapshot?.symbol_editor?.has_alternate ? "Already has one" : true;
      },
      run: (a) => a.symbolCommand("alternate_create"),
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
