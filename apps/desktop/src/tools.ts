import type { Mode } from "./bindings/Mode";

export interface ToolDef {
  id: string;
  label: string;
  /** lucide icon name, resolved in `ToolPalette`. */
  icon: string;
  shortcut?: string;
  /** Is the tool backed by working core functionality? Unimplemented tools stay visible, disabled. */
  implemented: boolean;
  /** Where it will come from, shown when unimplemented. */
  planned?: string;
}

const select: ToolDef = {
  id: "select",
  label: "Select",
  icon: "MousePointer2",
  shortcut: "Esc",
  implemented: true,
};

export const TOOLS: Partial<Record<Mode, ToolDef[]>> = {
  schematic: [select],
  pcb: [
    select,
    {
      id: "route",
      label: "Route Track",
      icon: "Route",
      shortcut: "X",
      implemented: false,
      planned: "roadmap stage 17 (tracks)",
    },
    {
      id: "via",
      label: "Place Via",
      icon: "CircleDot",
      shortcut: "V",
      implemented: false,
      planned: "roadmap stage 18 (vias)",
    },
  ],
};

export const MODE_TITLES: Record<Mode, string> = {
  schematic: "Schematic",
  pcb: "PCB",
  symbol_editor: "Symbol Editor",
  footprint_editor: "Footprint Editor",
  viewer3d: "3D Viewer",
};

/** Roadmap stage that will implement a mode that has no canvas yet. */
export const MODE_PLANNED: Partial<Record<Mode, string>> = {
  symbol_editor: "roadmap stage 09 (Symbol Editor)",
  footprint_editor: "roadmap stage 12 (Footprint Editor)",
  viewer3d: "roadmap stage 27 (PCB 3D)",
};
