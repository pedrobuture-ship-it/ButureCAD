import { getCurrentWindow } from "@tauri-apps/api/window";
import { open as openDialog } from "@tauri-apps/plugin-dialog";
import { api } from "./api";
import type { Snapshot } from "./bindings/Snapshot";
import type { Actions } from "./commands";
import { store } from "./store";
import { GRID_PRESETS } from "./units";

let toastTimer: ReturnType<typeof setTimeout> | undefined;

export function toast(text: string): void {
  store.set({ toast: text });
  clearTimeout(toastTimer);
  toastTimer = setTimeout(() => store.set({ toast: null }), 6000);
}

/** Installs a fresh snapshot from the core and asks the canvas to redraw. */
export function applySnapshot(s: Snapshot): void {
  const cur = store.getState();
  document.documentElement.dataset.theme = s.theme;
  const wantsRecovery = s.recovery_pending && cur.dialog?.kind !== "recovery";
  store.set({
    snapshot: s,
    hint: s.hint,
    renderTick: cur.renderTick + 1,
    dialog: wantsRecovery ? { kind: "recovery" } : cur.dialog,
  });
}

/** Runs a core call that returns a snapshot; failures become a toast and a refreshed snapshot. */
export async function call(p: Promise<Snapshot>): Promise<boolean> {
  try {
    applySnapshot(await p);
    return true;
  } catch (e) {
    toast(String(e));
    try {
      applySnapshot(await api.snapshot());
    } catch {
      /* the core is unreachable; the toast already says so */
    }
    return false;
  }
}

export async function refreshSnapshot(): Promise<void> {
  try {
    applySnapshot(await api.snapshot());
  } catch (e) {
    toast(String(e));
  }
}

/** Runs `then` immediately, or after the user decides what to do with unsaved changes. */
async function guardUnsaved(title: string, then: () => Promise<void>): Promise<void> {
  if (store.getState().snapshot?.project?.dirty) {
    store.set({ dialog: { kind: "unsaved", then, title } });
    return;
  }
  await then();
}

export async function pickFolder(title: string): Promise<string | null> {
  const picked = await openDialog({ directory: true, multiple: false, title });
  return typeof picked === "string" ? picked : null;
}

export async function openProjectAt(path: string): Promise<void> {
  await guardUnsaved("Open another project", async () => {
    await call(api.projectOpen(path));
  });
}

export async function closeWindowNow(): Promise<void> {
  await getCurrentWindow().destroy();
}

export function createActions(): Actions {
  const snap = () => store.getState().snapshot;
  return {
    newProject: () =>
      void guardUnsaved("Create a new project", async () =>
        store.set({ dialog: { kind: "new-project" } }),
      ),
    openProject: () =>
      void guardUnsaved("Open another project", async () => {
        const path = await pickFolder("Open ButureCAD project folder");
        if (path) await call(api.projectOpen(path));
      }),
    save: () => void call(api.projectSave()),
    saveAs: () => store.set({ dialog: { kind: "save-as" } }),
    closeProject: () =>
      void guardUnsaved("Close the project", async () => void (await call(api.projectClose(true)))),
    renameProject: () => store.set({ dialog: { kind: "rename-project" } }),
    undo: () => void call(api.undo()),
    redo: () => void call(api.redo()),
    selectAll: () => void call(api.editorCommand("select_all")),
    cancel: () => void call(api.editorCommand("cancel")),
    setMode: (mode) => void call(api.setMode(mode)),
    toggleTheme: () => void call(api.setTheme(snap()?.theme === "dark" ? "light" : "dark")),
    toggleUnits: () => void call(api.setUnits(snap()?.units === "mm" ? "mil" : "mm")),
    toggleGrid: () => void call(api.setGrid({ visible: !snap()?.grid.visible })),
    toggleSnapGrid: () => void call(api.setGrid({ snapGrid: !snap()?.grid.snap_grid })),
    toggleSnapObjects: () => void call(api.setGrid({ snapObjects: !snap()?.grid.snap_objects })),
    cycleGrid: () => {
      const sorted = [...GRID_PRESETS].sort((a, b) => a.nm - b.nm);
      const cur = snap()?.grid.spacing_nm ?? 0;
      const next = sorted.find((p) => p.nm > cur) ?? sorted[0];
      if (next) void call(api.setGrid({ spacingNm: next.nm }));
    },
    zoomFit: () => void call(api.viewFit()),
    zoomIn: () => void call(api.viewZoom(1.25)),
    zoomOut: () => void call(api.viewZoom(0.8)),
    toggleBottomPanel: () => store.set({ bottomOpen: !store.getState().bottomOpen }),
    toggleInspector: () => store.set({ inspectorOpen: !store.getState().inspectorOpen }),
    toggleDebugOverlay: () => void call(api.setDebugOverlay(!snap()?.debug_overlay)),
    openPalette: () => store.set({ paletteOpen: true }),
    showAbout: () => store.set({ dialog: { kind: "about" } }),
    selectTool: (id) => {
      if (snap()?.mode === "symbol_editor") {
        if (id === "text") store.set({ dialog: { kind: "symbol-text" } });
        else void call(api.editorCommand(`tool:${id}`));
        return;
      }
      if (id === "place_component" || id === "power") {
        store.set({ dialog: { kind: "symbol-picker", power: id === "power" } });
      } else if (id === "net_label" || id === "global_label") {
        store.set({ dialog: { kind: "label-name", global: id === "global_label" } });
      } else {
        void call(api.editorCommand(`tool:${id}`));
      }
    },
    editor: (name) => void call(api.editorCommand(name)),
    symbolNew: () => store.set({ dialog: { kind: "symbol-new" } }),
    symbolOpen: () => store.set({ dialog: { kind: "symbol-open" } }),
    symbolSaveToLibrary: () => store.set({ dialog: { kind: "symbol-save" } }),
    runErc: () => {
      store.set({ bottomTab: "erc", bottomOpen: true });
      void call(api.ercRun());
    },
    symbolCommand: (name, arg) => void call(api.symbolEditorCommand(name, arg)),
    startMove: () => {
      // With a selection, M carries it with the pointer; without one it arms the Move tool.
      const hasSel = (snap()?.selection.length ?? 0) > 0;
      void call(api.editorCommand(hasSel ? "start_move" : "tool:move"));
    },
  };
}
