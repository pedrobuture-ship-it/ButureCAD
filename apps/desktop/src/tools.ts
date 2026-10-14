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
  shortcut: "V",
  implemented: true,
};

const tool = (id: string, label: string, icon: string, shortcut?: string): ToolDef => ({
  id,
  label,
  icon,
  shortcut,
  implemented: true,
});

export const SYMBOL_TOOLS: ToolDef[] = [
  select,
  tool("pin", "Pin", "Pin", "P"),
  tool("line", "Line", "Slash", "L"),
  tool("rect", "Rectangle", "RectangleHorizontal", "B"),
  tool("circle", "Circle", "Circle", "C"),
  tool("arc", "Arc", "Spline", "A"),
  tool("text", "Text", "Type", "T"),
];

export const FOOTPRINT_TOOLS: ToolDef[] = [
  select,
  tool("pad_smd", "SMD Pad", "RectangleHorizontal", "S"),
  tool("pad_through", "Through-hole Pad", "CircleDot", "H"),
  tool("npth", "Non-plated Hole", "Crosshair", "N"),
  tool("line", "Line", "Slash", "L"),
  tool("rect", "Rectangle", "Square", "B"),
  tool("circle", "Circle", "Circle", "C"),
  tool("arc", "Arc", "Spline", "A"),
  tool("text", "Text", "Type", "T"),
];

export const TOOLS: Partial<Record<Mode, ToolDef[]>> = {
  schematic: [
    select,
    tool("place_component", "Place Component", "Cpu", "A"),
    tool("wire", "Wire", "PenLine", "W"),
    tool("bus", "Bus", "Cable", "B"),
    tool("junction", "Junction", "CircleDot", "J"),
    tool("net_label", "Net Label", "Tag", "L"),
    tool("global_label", "Global Label", "Globe", "Ctrl+L"),
    tool("power", "Power Symbol", "Zap", "P"),
    tool("move", "Move", "Move", "M"),
    tool("drag", "Drag", "Hand", "G"),
    tool("rotate", "Rotate", "RotateCw", "R"),
    tool("mirror", "Mirror", "FlipHorizontal", "X / Y"),
    tool("delete", "Delete", "Trash2", "Del"),
    tool("duplicate", "Duplicate", "Copy", "Ctrl+D"),
  ],
  symbol_editor: SYMBOL_TOOLS,
  footprint_editor: FOOTPRINT_TOOLS,
  pcb: [
    select,
    tool("place_footprint", "Place Footprint", "Cpu", "A"),
    tool("outline_rect", "Board Outline (rectangle)", "Square", "O"),
    tool("outline_line", "Board Outline (polyline)", "PenLine"),
    tool("route", "Route Track", "Route", "X"),
    tool("route_pair", "Route Differential Pair", "Equal", "Shift+X"),
    tool("via", "Place Via", "CircleSlash", "V"),
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
  viewer3d: "roadmap stage 27 (PCB 3D)",
};
