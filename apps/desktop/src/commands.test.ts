import { beforeEach, describe, expect, it } from "vitest";
import { availability, buildCommands, filterCommands, fuzzyScore, type Actions } from "./commands";
import { matchesShortcut, parseShortcut } from "./shortcuts";
import { store, type UiState } from "./store";
import type { Snapshot } from "./bindings/Snapshot";

const commands = buildCommands();

/** Commands of the symbol, footprint and PCB editors may reuse keys of other modes. */
const modeSpecific = (id: string) => /^(symtool|fptool|pcbtool|pcb)\./.test(id);

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
    symbol_editor: null,
    erc: null,
    drc: null,
    fab: null,
    gerber: null,
    library_manager: null,
    plugins: null,
    simulation: null,
    code: null,
    viewer3d: null,
    footprint_editor: null,
    pcb: null,
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

  it("has unique ids and unique shortcuts (Symbol Editor tools may reuse keys)", () => {
    const ids = commands.map((c) => c.id);
    expect(new Set(ids).size).toBe(ids.length);
    // Symbol Editor tools reuse letters of schematic tools; the Symbol Editor describe block
    // below checks that the two are never available at the same time.
    const shortcuts = commands.flatMap((c) =>
      c.shortcut && !modeSpecific(c.id) ? [c.shortcut.toLowerCase()] : [],
    );
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

describe("Symbol Editor commands", () => {
  const withSymbol = (over: Partial<Snapshot> = {}) =>
    snapshot({
      mode: "symbol_editor",
      symbol_editor: {
        lib_id: "User:X",
        unit: 1,
        unit_count: 1,
        representation: 1,
        has_alternate: false,
        used_by: 0,
        pin_template: [],
        fields: [],
        custom: [],
        warnings: [],
      },
      ...over,
    });
  const withFootprint = () =>
    snapshot({
      mode: "footprint_editor",
      footprint_editor: {
        name: "User:F",
        layer: "silkscreen",
        pad_count: 0,
        pad_template: [],
        fields: [],
        models: [],
        warnings: [],
        generators: [],
      },
    });
  const withPcb = () =>
    snapshot({
      mode: "pcb",
      pcb: {
        copper_layers: 2,
        active_layer: "F.Cu",
        footprints: 0,
        nets: 0,
        warnings: [],
        ratsnest_visible: true,
        unrouted: 0,
        length: null,
        zone_drawing: false,
        zone_net: null,
        auto_fill: true,
        stale_zones: 0,
        zones: 0,
        net_choices: [],
        route_width_mm: "0.25",
        route_width_auto: true,
        route_obstacles: "walk",
        route_class: "Default",
        via_size_auto: true,
        route_mode: "45",
        via_size: "0.8/0.4",
        route: null,
      },
    });
  const state = (snap: Snapshot): UiState => ({ ...store.getState(), snapshot: snap });
  const byId = (id: string) => {
    const c = commands.find((x) => x.id === id);
    if (!c) throw new Error(`missing command ${id}`);
    return c;
  };

  it("symbol tools need an open symbol in the Symbol Editor", () => {
    expect(availability(byId("symtool.pin"), state(snapshot()))).not.toBe(true);
    expect(availability(byId("symtool.pin"), state(withSymbol({ symbol_editor: null })))).not.toBe(
      true,
    );
    expect(availability(byId("symtool.pin"), state(withSymbol()))).toBe(true);
  });

  it("shares keys with other modes without ever being available together", () => {
    for (const sym of commands.filter((c) => modeSpecific(c.id) && c.shortcut)) {
      const clashes = commands.filter(
        (c) => c !== sym && c.shortcut?.toLowerCase() === sym.shortcut?.toLowerCase(),
      );
      for (const other of clashes) {
        for (const snap of [snapshot(), withSymbol(), withFootprint(), withPcb()]) {
          const both =
            availability(sym, state(snap)) === true && availability(other, state(snap)) === true;
          expect(both, `${sym.id} vs ${other.id} in ${snap.mode}`).toBe(false);
        }
      }
    }
  });

  it("rotate and mirror work in both editors, schematic-only commands do not", () => {
    const st = state(
      withSymbol({ selection: [{ id: 1, kind: "pin", title: "p", properties: [] }] }),
    );
    expect(availability(byId("edit.rotate"), st)).toBe(true);
    expect(availability(byId("edit.delete"), st)).toBe(true);
    expect(availability(byId("edit.duplicate"), st)).not.toBe(true);
  });

  it("footprint tools need an open footprint in the Footprint Editor", () => {
    const pad = byId("fptool.pad_smd");
    expect(availability(pad, state(snapshot()))).not.toBe(true);
    expect(availability(pad, state(withSymbol()))).not.toBe(true);
    expect(availability(pad, state(withFootprint()))).toBe(true);
    // Rotate / delete work in the Footprint Editor too.
    const sel = [{ id: 1, kind: "pad", title: "p", properties: [] }];
    expect(availability(byId("edit.rotate"), state({ ...withFootprint(), selection: sel }))).toBe(
      true,
    );
  });
});
