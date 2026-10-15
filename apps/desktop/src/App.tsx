import { getCurrentWindow } from "@tauri-apps/api/window";
import { useEffect } from "react";
import { api } from "./api";
import { call, closeWindowNow, refreshSnapshot, toast } from "./actions";
import { availability } from "./commands";
import { BottomPanel } from "./components/BottomPanel";
import { CanvasView } from "./components/CanvasView";
import { CodePanel } from "./components/CodePanel";
import { SimulationPanel } from "./components/SimulationPanel";
import { PluginsPanel } from "./components/PluginsPanel";
import { LibraryManager } from "./components/LibraryManager";
import { GerberViewerBar } from "./components/GerberViewerBar";
import { Viewer3dBar } from "./components/Viewer3dBar";
import { CommandPalette } from "./components/CommandPalette";
import { ContextMenu } from "./components/ContextMenu";
import { Dialogs } from "./components/Dialogs";
import { DocTabs } from "./components/DocTabs";
import { useActions, useCommands } from "./components/hooks";
import { Inspector } from "./components/Inspector";
import { MenuBar } from "./components/MenuBar";
import { NotImplemented } from "./components/NotImplemented";
import { FootprintEditorBar, FootprintEditorHome } from "./components/FootprintEditor";
import { PcbBar } from "./components/PcbBar";
import { SymbolEditorBar, SymbolEditorHome } from "./components/SymbolEditor";
import { StartScreen } from "./components/StartScreen";
import { StatusBar } from "./components/StatusBar";
import { Toolbar } from "./components/Toolbar";
import { ToolPalette } from "./components/ToolPalette";
import { matchesShortcut, parseShortcut } from "./shortcuts";
import { store, useUi } from "./store";

const AUTOSAVE_MS = 30_000;

function isTyping(t: EventTarget | null): boolean {
  return (
    t instanceof HTMLInputElement ||
    t instanceof HTMLTextAreaElement ||
    (t instanceof HTMLElement && t.isContentEditable)
  );
}

export function App() {
  const snap = useUi((s) => s.snapshot);
  const inspectorOpen = useUi((s) => s.inspectorOpen);
  const toastText = useUi((s) => s.toast);
  const commands = useCommands();
  const actions = useActions();

  // Boot: follow the OS theme, then load the first snapshot.
  useEffect(() => {
    const dark = window.matchMedia("(prefers-color-scheme: dark)").matches;
    void call(api.setTheme(dark ? "dark" : "light"));
  }, []);

  // Autosave: the core only writes when there are unsaved, not-yet-autosaved changes.
  useEffect(() => {
    const id = setInterval(() => {
      api
        .autosaveTick()
        .then(async (wrote) => {
          if (wrote) await refreshSnapshot();
        })
        .catch((e) => toast(`Autosave failed: ${String(e)}`));
    }, AUTOSAVE_MS);
    return () => clearInterval(id);
  }, []);

  // Keyboard shortcuts come from the command registry.
  useEffect(() => {
    const parsed = commands.map((c) => ({ c, s: c.shortcut ? parseShortcut(c.shortcut) : null }));
    const onKey = (e: KeyboardEvent) => {
      const state = store.getState();
      if (state.dialog || (isTyping(e.target) && e.key !== "Escape" && !(e.ctrlKey || e.metaKey)))
        return;
      if (state.paletteOpen) return;
      // Several commands may share a key in different modes (P is Power in the schematic and
      // Pin in the Symbol Editor): run the first one that is available right now.
      for (const { c, s } of parsed) {
        if (s && matchesShortcut(e, s)) {
          if (
            isTyping(e.target) &&
            (c.id === "edit.undo" || c.id === "edit.redo" || c.id === "edit.selectAll")
          )
            return; // text fields keep native editing
          if (availability(c, state) === true) {
            e.preventDefault();
            c.run(actions);
            return;
          }
        }
      }
    };
    window.addEventListener("keydown", onKey);
    return () => window.removeEventListener("keydown", onKey);
  }, [commands, actions]);

  // Closing the window with unsaved changes asks first.
  useEffect(() => {
    let unlisten: (() => void) | undefined;
    void getCurrentWindow()
      .onCloseRequested((event) => {
        if (store.getState().snapshot?.project?.dirty) {
          event.preventDefault();
          store.set({
            dialog: { kind: "unsaved", title: "Quit ButureCAD", then: closeWindowNow },
          });
        }
      })
      .then((u) => {
        unlisten = u;
      });
    return () => unlisten?.();
  }, []);

  // Window title reflects the project and its unsaved state.
  useEffect(() => {
    const p = snap?.project;
    const title = p ? `${p.name}${p.dirty ? " •" : ""} — ButureCAD` : "ButureCAD";
    document.title = title;
    void getCurrentWindow().setTitle(title);
  }, [snap?.project?.name, snap?.project?.dirty, snap?.project]);

  const mode = snap?.mode ?? "schematic";
  const hasCanvasMode =
    mode === "schematic" ||
    mode === "pcb" ||
    (mode === "symbol_editor" && !!snap?.symbol_editor) ||
    (mode === "footprint_editor" && !!snap?.footprint_editor) ||
    mode === "viewer3d" ||
    mode === "gerber_viewer";

  return (
    <div className={inspectorOpen ? "app" : "app no-inspector"}>
      <MenuBar />
      <Toolbar />
      <div className="workspace">
        <ToolPalette />
        <div className="center">
          <DocTabs />
          <div className="stage">
            {!snap?.project ? (
              <StartScreen />
            ) : mode === "symbol_editor" && !snap.symbol_editor ? (
              <SymbolEditorHome />
            ) : mode === "footprint_editor" && !snap.footprint_editor ? (
              <FootprintEditorHome />
            ) : mode === "code" ? (
              <CodePanel />
            ) : mode === "simulation" ? (
              <SimulationPanel />
            ) : mode === "plugins" ? (
              <PluginsPanel />
            ) : mode === "library_manager" ? (
              <LibraryManager />
            ) : hasCanvasMode ? (
              <>
                {mode === "symbol_editor" && <SymbolEditorBar />}
                {mode === "footprint_editor" && <FootprintEditorBar />}
                {mode === "pcb" && <PcbBar />}
                {mode === "viewer3d" && <Viewer3dBar />}
                {mode === "gerber_viewer" && <GerberViewerBar />}
                <CanvasView />
              </>
            ) : (
              <NotImplemented mode={mode} />
            )}
          </div>
          <BottomPanel />
        </div>
        {inspectorOpen && <Inspector />}
      </div>
      <StatusBar />
      <ContextMenu />
      <CommandPalette />
      <Dialogs />
      {toastText && (
        <div className="toast" role="alert" onClick={() => store.set({ toast: null })}>
          {toastText}
        </div>
      )}
    </div>
  );
}
