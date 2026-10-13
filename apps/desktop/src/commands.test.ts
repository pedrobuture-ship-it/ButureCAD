import { beforeEach, describe, expect, it } from "vitest";
import { availability, buildCommands, filterCommands, fuzzyScore, type Actions } from "./commands";
import { matchesShortcut, parseShortcut } from "./shortcuts";
import { store, type UiState } from "./store";
import type { Snapshot } from "./bindings/Snapshot";

const commands = buildCommands();

function snapshot(over: Partial<Snapshot> = {}): Snapshot {
  return {
    project: {
      name: "P",
      path: "/p",
      dirty: false,
      can_undo: false,
      can_redo: false,
      undo_history: [],
      redo_history: [],
    },
    mode: "schematic",
    tool: "select",
    tool_arg: null,
    can_paste: false,
    units: "mm",
    theme: "dark",
    grid: { spacing_nm: 1_270_000, visible: true, snap_grid: true, snap_objects: true },
    layers: [],
    selection: [],
    gpu_adapter: null,
    gpu_error: null,
    debug_overlay: false,
    recent: [],
    recovery_pending: false,
    messages: [],
    command_log: [],
    object_count: 0,
    zoom_percent: 100,
    hint: "",
    ...over,
  };
}

function state(snap: Snapshot | null): UiState {
  return { ...store.getState(), snapshot: snap };
}

const find = (id: string) => {
  const c = commands.find((x) => x.id === id);
  if (!c) throw new Error(`missing command ${id}`);
  return c;
};

describe("command registry", () => {
  beforeEach(() => store.reset());

  it("has unique ids and unique shortcuts", () => {
    const ids = commands.map((c) => c.id);
    expect(new Set(ids).size).toBe(ids.length);
    const shortcuts = commands.flatMap((c) => (c.shortcut ? [c.shortcut.toLowerCase()] : []));
    expect(new Set(shortcuts).size).toBe(shortcuts.length);
  });

  it("every shortcut parses and matches its own key event", () => {
    for (const c of commands.filter((x) => x.shortcut)) {
      const s = parseShortcut(c.shortcut ?? "");
      const ev = { key: s.key, ctrlKey: s.ctrl, shiftKey: s.shift, altKey: s.alt, metaKey: false };
      expect(matchesShortcut(ev, s), c.id).toBe(true);
    }
  });

  it("canvas commands are unavailable without a project, with a reason", () => {
    expect(availability(find("view.fit"), state(null))).toBe("Open or create a project first");
    expect(availability(find("view.fit"), state(snapshot()))).toBe(true);
    expect(availability(find("file.save"), state(null))).toBe("No project is open");
  });

  it("canvas commands are unavailable in modes that have no canvas", () => {
    const s = state(snapshot({ mode: "viewer3d" }));
    expect(availability(find("view.fit"), s)).toBe("This mode has no canvas yet");
    expect(availability(find("mode.pcb"), s)).toBe(true);
  });

  it("undo/redo availability follows the engine state", () => {
    const base = snapshot();
    const p = base.project;
    if (!p) throw new Error("fixture has a project");
    expect(availability(find("edit.undo"), state(base))).toBe("Nothing to undo");
    expect(
      availability(find("edit.undo"), state({ ...base, project: { ...p, can_undo: true } })),
    ).toBe(true);
    expect(
      availability(find("edit.redo"), state({ ...base, project: { ...p, can_redo: true } })),
    ).toBe(true);
  });

  it("commands call the matching action", () => {
    const called: string[] = [];
    const actions = new Proxy({} as Actions, {
      get:
        (_t, name: string) =>
        (...args: unknown[]) =>
          called.push(`${name}(${args.join(",")})`),
    });
    find("file.save").run(actions);
    find("mode.pcb").run(actions);
    find("view.theme").run(actions);
    expect(called).toEqual(["save()", "setMode(pcb)", "toggleTheme()"]);
  });
});

describe("command palette search", () => {
  it("matches subsequences and ranks prefix/word matches first", () => {
    const names = (q: string) => filterCommands(commands, q).map((c) => c.id);
    expect(names("save")[0]).toBe("file.save");
    expect(names("save")).toContain("file.saveAs");
    expect(names("zfit")).toContain("view.fit");
    expect(names("pcb")[0]).toBe("mode.pcb");
    expect(names("qqqq")).toEqual([]);
  });

  it("an empty query lists everything in registry order", () => {
    expect(filterCommands(commands, "").map((c) => c.id)).toEqual(commands.map((c) => c.id));
  });

  it("fuzzy scoring", () => {
    expect(fuzzyScore("sv", "Save")).not.toBeNull();
    expect(fuzzyScore("vs", "Save")).toBeNull();
    expect((fuzzyScore("sa", "Save") ?? 0) > (fuzzyScore("sa", "Cases") ?? 0)).toBe(true);
  });
});
