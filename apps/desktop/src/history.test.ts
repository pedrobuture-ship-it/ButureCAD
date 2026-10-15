import { beforeEach, describe, expect, it } from "vitest";
import {
  buildCommands,
  filterCommands,
  isRepeatable,
  isTool,
  previousToolTarget,
  repeatTarget,
  availability,
  type AppCommand,
} from "./commands";
import { CommandHistory, type KeyValueStorage } from "./history";
import { store, type UiState } from "./store";
import type { Snapshot } from "./bindings/Snapshot";

const commands = buildCommands();
const find = (id: string): AppCommand => {
  const c = commands.find((x) => x.id === id);
  if (!c) throw new Error(`missing ${id}`);
  return c;
};

function memory(): KeyValueStorage & { data: Map<string, string> } {
  const data = new Map<string, string>();
  return { data, getItem: (k) => data.get(k) ?? null, setItem: (k, v) => void data.set(k, v) };
}

/** A schematic project with one selected object, so editing commands are available. */
function schematicState(selected = 1): UiState {
  const snapshot = {
    project: { name: "P", path: "/p", dirty: false, can_undo: false, can_redo: false },
    mode: "schematic",
    tool: "select",
    selection: Array.from({ length: selected }, (_, i) => ({ id: i, title: "R1" })),
    can_paste: false,
  } as unknown as Snapshot;
  return { ...store.getState(), snapshot };
}

describe("CommandHistory", () => {
  it("keeps the most recent first, without duplicates", () => {
    const h = new CommandHistory(memory());
    h.record("a");
    h.record("b");
    h.record("a");
    expect(h.recent()).toEqual(["a", "b"]);
  });

  it("is bounded", () => {
    const h = new CommandHistory(memory());
    for (let i = 0; i < 100; i++) h.record(`c${i}`);
    expect(h.recent().length).toBe(40);
    expect(h.recent()[0]).toBe("c99");
  });

  it("persists and reloads", () => {
    const st = memory();
    new CommandHistory(st).record("edit.rotate");
    expect(new CommandHistory(st).recent()).toEqual(["edit.rotate"]);
  });

  it("survives damaged or unavailable storage", () => {
    const bad = memory();
    bad.data.set("buturecad.commandHistory.v1", "{not json");
    expect(new CommandHistory(bad).recent()).toEqual([]);
    const wrongShape = memory();
    wrongShape.data.set("buturecad.commandHistory.v1", JSON.stringify([1, "x", null, { a: 1 }]));
    expect(new CommandHistory(wrongShape).recent()).toEqual(["x"]);
    const throwing: KeyValueStorage = {
      getItem: () => {
        throw new Error("blocked");
      },
      setItem: () => {
        throw new Error("blocked");
      },
    };
    const h = new CommandHistory(throwing);
    h.record("a"); // must not throw
    expect(h.recent()).toEqual(["a"]);
    const none = new CommandHistory(null);
    none.record("b");
    expect(none.recent()).toEqual(["b"]);
  });
});

describe("repeat last command", () => {
  beforeEach(() => store.reset());

  it("classifies what can be repeated", () => {
    expect(isRepeatable(find("edit.rotate"))).toBe(true);
    expect(isRepeatable(find("edit.duplicate"))).toBe(true);
    expect(isRepeatable(find("tool.wire"))).toBe(true);
    expect(isRepeatable(find("tools.erc"))).toBe(true);
    for (const id of [
      "edit.undo",
      "edit.redo",
      "edit.repeat",
      "edit.cancel",
      "edit.selectAll",
      "tool.previous",
      "tools.palette",
      "file.save",
      "view.fit",
      "mode.pcb",
      "help.about",
    ]) {
      expect(isRepeatable(find(id)), id).toBe(false);
    }
  });

  it("picks the most recent repeatable command that is available now", () => {
    const s = schematicState();
    // Undo is never repeated; file.save is not an editing command.
    expect(
      repeatTarget(commands, s, ["edit.undo", "file.save", "edit.rotate", "edit.delete"])?.id,
    ).toBe("edit.rotate");
    expect(repeatTarget(commands, s, [])).toBeNull();
    // With nothing selected "delete" is unavailable, so the repeat falls through to a usable one.
    const none = schematicState(0);
    expect(availability(find("edit.delete"), none)).not.toBe(true);
    expect(repeatTarget(commands, none, ["edit.delete", "tool.wire"])?.id).toBe("tool.wire");
  });

  it("ignores ids that no longer exist", () => {
    expect(repeatTarget(commands, schematicState(), ["gone.command", "edit.rotate"])?.id).toBe(
      "edit.rotate",
    );
  });
});

describe("previous tool", () => {
  beforeEach(() => store.reset());

  it("recognises tools", () => {
    expect(isTool(find("tool.wire"))).toBe(true);
    expect(isTool(find("pcbtool.route"))).toBe(true);
    expect(isTool(find("tool.previous"))).toBe(false);
    expect(isTool(find("edit.rotate"))).toBe(false);
  });

  it("goes back to the tool used before the current one, alternating", () => {
    const s = schematicState();
    // newest first: wire is current, select was used before it
    expect(previousToolTarget(commands, s, ["tool.wire", "tool.select", "tool.bus"])?.id).toBe(
      "tool.select",
    );
    // after switching back, wire becomes the previous one
    expect(previousToolTarget(commands, s, ["tool.select", "tool.wire", "tool.bus"])?.id).toBe(
      "tool.wire",
    );
  });

  it("skips tools that do not work in the current mode and non-tool commands", () => {
    const s = schematicState();
    expect(
      previousToolTarget(commands, s, ["tool.wire", "edit.rotate", "pcbtool.route", "tool.bus"])
        ?.id,
    ).toBe("tool.bus");
    expect(previousToolTarget(commands, s, ["tool.wire"])).toBeNull();
  });
});

describe("palette ordering with history", () => {
  it("lists recent commands first when nothing is typed", () => {
    const ids = filterCommands(commands, "", ["edit.rotate", "view.fit"]).map((c) => c.id);
    expect(ids.slice(0, 2)).toEqual(["edit.rotate", "view.fit"]);
    expect(ids.length).toBe(commands.length);
    expect(new Set(ids).size).toBe(commands.length);
  });

  it("a search keeps the best match first even if another command is recent", () => {
    const ids = filterCommands(commands, "save", ["edit.rotate"]).map((c) => c.id);
    expect(ids[0]).toBe("file.save");
  });

  it("recent commands win ties between equal matches", () => {
    const base = filterCommands(commands, "align", []).map((c) => c.id);
    const last = base[base.length - 1] ?? "";
    expect(filterCommands(commands, "align", [last])[0]?.id).toBe(last);
  });
});

describe("keyboard shortcuts per view", () => {
  const modes = ["schematic", "pcb", "symbol_editor", "footprint_editor"] as const;
  const stubs = {
    symbol_editor: { lib_id: "U:X", unit: 1, unit_count: 1, warnings: [] },
    footprint_editor: { name: "U:F", warnings: [] },
    pcb: { copper_layers: 2, zones: 1, route: { net: "N" }, zone_drawing: false },
  };

  it("no two commands are available on the same key at the same time", () => {
    const conflicts: string[] = [];
    for (const mode of modes) {
      for (const selected of [0, 2]) {
        const snapshot = {
          project: { name: "P", path: "/p", dirty: false, can_undo: true, can_redo: true },
          mode,
          tool: "select",
          selection: Array.from({ length: selected }, (_, i) => ({ id: i, title: "x" })),
          can_paste: true,
          symbol_editor: mode === "symbol_editor" ? stubs.symbol_editor : null,
          footprint_editor: mode === "footprint_editor" ? stubs.footprint_editor : null,
          pcb: mode === "pcb" ? stubs.pcb : null,
        } as unknown as Snapshot;
        const s: UiState = { ...store.getState(), snapshot };
        const byKey = new Map<string, string[]>();
        for (const c of commands) {
          if (!c.shortcut || availability(c, s) !== true) continue;
          const k = c.shortcut.toLowerCase();
          byKey.set(k, [...(byKey.get(k) ?? []), c.id]);
        }
        for (const [k, ids] of byKey) {
          if (ids.length > 1)
            conflicts.push(`${mode}/${selected} selected: ${k} → ${ids.join(", ")}`);
        }
      }
    }
    expect(conflicts).toEqual([]);
  });
});

describe("commands the PCB editor does not implement say so", () => {
  it("copy and cut are unavailable in the PCB editor, with a reason", () => {
    const snapshot = {
      project: { name: "P", path: "/p", dirty: false, can_undo: false, can_redo: false },
      mode: "pcb",
      tool: "select",
      selection: [{ id: 1, title: "R1" }],
      can_paste: true,
      pcb: { copper_layers: 2, zones: 0, route: null, zone_drawing: false },
    } as unknown as Snapshot;
    const s: UiState = { ...store.getState(), snapshot };
    for (const id of ["edit.copy", "edit.cut", "edit.paste"]) {
      expect(availability(find(id), s), id).not.toBe(true);
    }
    // Duplicate does work on the board.
    expect(availability(find("edit.duplicate"), s)).toBe(true);
  });
});
