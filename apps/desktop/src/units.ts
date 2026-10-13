import type { Units } from "./bindings/Units";

export const NM_PER_MM = 1_000_000;
export const NM_PER_MIL = 25_400;

/** Formats a nanometre length in the display unit (display only: the core stays in nm). */
export function formatLength(nm: number, units: Units, digits?: number): string {
  if (units === "mm") return (nm / NM_PER_MM).toFixed(digits ?? 3);
  return (nm / NM_PER_MIL).toFixed(digits ?? 1);
}

export function unitLabel(units: Units): string {
  return units === "mm" ? "mm" : "mil";
}

export interface GridPreset {
  nm: number;
  label: string;
}

/** Common schematic/PCB grids (all exact in nm). */
export const GRID_PRESETS: GridPreset[] = [
  { nm: 25_400, label: "1 mil" },
  { nm: 127_000, label: "5 mil" },
  { nm: 254_000, label: "10 mil" },
  { nm: 635_000, label: "25 mil" },
  { nm: 1_270_000, label: "50 mil" },
  { nm: 2_540_000, label: "100 mil" },
  { nm: 100_000, label: "0.1 mm" },
  { nm: 500_000, label: "0.5 mm" },
  { nm: 1_000_000, label: "1 mm" },
];

export function gridLabel(nm: number, units: Units): string {
  const preset = GRID_PRESETS.find((p) => p.nm === nm);
  if (preset) return preset.label;
  return `${formatLength(nm, units)} ${unitLabel(units)}`;
}
