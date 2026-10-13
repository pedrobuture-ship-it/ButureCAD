import { describe, expect, it } from "vitest";
import { GRID_PRESETS, formatLength, gridLabel } from "./units";

describe("units", () => {
  it("formats nanometres in mm and mil", () => {
    expect(formatLength(1_270_000, "mm")).toBe("1.270");
    expect(formatLength(1_270_000, "mil")).toBe("50.0");
    expect(formatLength(-2_540_000, "mm", 2)).toBe("-2.54");
  });

  it("labels known grids and falls back to a formatted length", () => {
    expect(gridLabel(1_270_000, "mm")).toBe("50 mil");
    expect(gridLabel(333_000, "mm")).toBe("0.333 mm");
    expect(gridLabel(333_000, "mil")).toBe("13.1 mil");
  });

  it("presets are unique and positive", () => {
    const nms = GRID_PRESETS.map((p) => p.nm);
    expect(new Set(nms).size).toBe(nms.length);
    expect(nms.every((n) => n > 0)).toBe(true);
  });
});
