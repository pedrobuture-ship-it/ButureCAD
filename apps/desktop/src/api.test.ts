import { describe, expect, it } from "vitest";
import type { CoreInfo } from "./api";

describe("api contract", () => {
  it("CoreInfo has name and version", () => {
    const info: CoreInfo = { name: "ButureCAD", version: "0.1.0" };
    expect(info.name).toBe("ButureCAD");
  });
});
