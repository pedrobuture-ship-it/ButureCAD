import { useEffect, useState, type ReactNode } from "react";
import { api } from "../api";
import { call, pickFolder, toast } from "../actions";
import type { SymbolInfo } from "../bindings/SymbolInfo";
import type { CoreInfo } from "../bindings/CoreInfo";
import { store, useUi } from "../store";

function Modal({
  title,
  children,
  onClose,
}: {
  title: string;
  children: ReactNode;
  onClose: () => void;
}) {
  useEffect(() => {
    const onKey = (e: KeyboardEvent) => e.key === "Escape" && onClose();
    window.addEventListener("keydown", onKey);
    return () => window.removeEventListener("keydown", onKey);
  }, [onClose]);
  return (
    <div className="overlay" onMouseDown={onClose}>
      <div
        className="modal"
        role="dialog"
        aria-label={title}
        onMouseDown={(e) => e.stopPropagation()}
      >
        <h2>{title}</h2>
        {children}
      </div>
    </div>
  );
}

const close = () => store.set({ dialog: null });

function FolderAndName({ mode }: { mode: "new-project" | "save-as" }) {
  const [name, setName] = useState(
    mode === "new-project" ? "Untitled" : (store.getState().snapshot?.project?.name ?? "Copy"),
  );
  const [parent, setParent] = useState<string | null>(null);
  const valid = name.trim().length > 0 && !/[\\/]/.test(name) && parent !== null;
  const submit = async () => {
    if (!valid || parent === null) return;
    close();
    if (mode === "new-project") await call(api.projectNew(parent, name.trim()));
    else await call(api.projectSaveAs(`${parent}/${name.trim()}`));
  };
  return (
    <Modal title={mode === "new-project" ? "New project" : "Save a copy as…"} onClose={close}>
      <label className="field">
        <span>
          {mode === "new-project" ? "Project name (also the folder name)" : "New folder name"}
        </span>
        <input
          autoFocus
          value={name}
          onChange={(e) => setName(e.target.value)}
          onKeyDown={(e) => e.key === "Enter" && void submit()}
        />
      </label>
      <div className="field">
        <span>Location</span>
        <div className="row">
          <input readOnly value={parent ?? ""} placeholder="Choose a parent folder…" />
          <button
            onClick={async () => {
              try {
                const p = await pickFolder("Choose the parent folder");
                if (p) setParent(p);
              } catch (e) {
                toast(String(e));
              }
            }}
          >
            Browse…
          </button>
        </div>
      </div>
      {name.trim() !== "" && /[\\/]/.test(name) && (
        <p className="warn">The name must not contain / or \</p>
      )}
      <div className="actions">
        <button onClick={close}>Cancel</button>
        <button className="primary" disabled={!valid} onClick={() => void submit()}>
          {mode === "new-project" ? "Create" : "Save copy"}
        </button>
      </div>
    </Modal>
  );
}

function RenameDialog() {
  const [name, setName] = useState(store.getState().snapshot?.project?.name ?? "");
  const submit = async () => {
    close();
    await call(api.projectRename(name.trim()));
  };
  return (
    <Modal title="Rename project" onClose={close}>
      <label className="field">
        <span>Project name</span>
        <input
          autoFocus
          value={name}
          onChange={(e) => setName(e.target.value)}
          onKeyDown={(e) => e.key === "Enter" && name.trim() && void submit()}
        />
      </label>
      <p className="muted">Renaming is an undoable edit; the folder keeps its name.</p>
      <div className="actions">
        <button onClick={close}>Cancel</button>
        <button className="primary" disabled={!name.trim()} onClick={() => void submit()}>
          Rename
        </button>
      </div>
    </Modal>
  );
}

function UnsavedDialog({ title, then }: { title: string; then: () => Promise<void> }) {
  return (
    <Modal title={title} onClose={close}>
      <p>The project has unsaved changes.</p>
      <div className="actions">
        <button onClick={close}>Cancel</button>
        <button
          className="danger"
          onClick={async () => {
            close();
            // Closing first lets the next step replace the project without the unsaved-changes guard.
            if (await call(api.projectClose(true))) await then();
          }}
        >
          Discard changes
        </button>
        <button
          className="primary"
          onClick={async () => {
            close();
            if (await call(api.projectSave())) await then();
          }}
        >
          Save
        </button>
      </div>
    </Modal>
  );
}

function RecoveryDialog() {
  return (
    <Modal title="Recover unsaved work?" onClose={() => undefined}>
      <p>
        ButureCAD did not close cleanly last time and found an autosave newer than the saved
        project.
      </p>
      <div className="actions">
        <button onClick={async () => (close(), void (await call(api.projectRecovery(false))))}>
          Discard autosave
        </button>
        <button
          className="primary"
          onClick={async () => (close(), void (await call(api.projectRecovery(true))))}
        >
          Recover
        </button>
      </div>
    </Modal>
  );
}

function SymbolPicker({ power }: { power: boolean }) {
  const [symbols, setSymbols] = useState<SymbolInfo[]>([]);
  const [query, setQuery] = useState("");
  const [index, setIndex] = useState(0);
  useEffect(() => {
    api
      .librarySymbols()
      .then(setSymbols)
      .catch((e) => toast(String(e)));
  }, []);
  const q = query.trim().toLowerCase();
  const shown = symbols.filter(
    (s) => s.is_power === power && (q === "" || s.lib_id.toLowerCase().includes(q)),
  );
  const pick = async (s: SymbolInfo | undefined) => {
    if (!s) return;
    close();
    await call(api.editorCommand(`tool:${power ? "power" : "place_component"}:${s.lib_id}`));
  };
  return (
    <Modal title={power ? "Choose a power symbol" : "Choose a component"} onClose={close}>
      <input
        autoFocus
        value={query}
        placeholder="Search the library…"
        onChange={(e) => {
          setQuery(e.target.value);
          setIndex(0);
        }}
        onKeyDown={(e) => {
          if (e.key === "ArrowDown") setIndex((i) => Math.min(shown.length - 1, i + 1));
          else if (e.key === "ArrowUp") setIndex((i) => Math.max(0, i - 1));
          else if (e.key === "Enter") void pick(shown[index]);
        }}
      />
      <ul className="symbol-list" role="listbox">
        {shown.length === 0 && <li className="muted">No matching symbol</li>}
        {shown.map((s, i) => (
          <li
            key={s.lib_id}
            role="option"
            aria-selected={i === index}
            className={i === index ? "sel" : ""}
            onMouseEnter={() => setIndex(i)}
            onClick={() => void pick(s)}
          >
            <strong>{s.name}</strong>
            <span className="muted">
              {s.lib_id.split(":")[0]} · {s.pin_count} pin{s.pin_count === 1 ? "" : "s"}
              {s.reference_prefix ? ` · ${s.reference_prefix}` : ""}
            </span>
          </li>
        ))}
      </ul>
      <p className="muted">
        Click to arm the tool, then click on the sheet to place. R rotates, X/Y mirror, Esc stops.
      </p>
    </Modal>
  );
}

function LabelNameDialog({ global }: { global: boolean }) {
  const [name, setName] = useState("NET1");
  const submit = async () => {
    if (!name.trim()) return;
    close();
    await call(api.editorCommand(`tool:${global ? "global_label" : "net_label"}:${name.trim()}`));
  };
  return (
    <Modal title={global ? "Global label" : "Net label"} onClose={close}>
      <label className="field">
        <span>Name (a trailing number counts up as you place more labels)</span>
        <input
          autoFocus
          value={name}
          onChange={(e) => setName(e.target.value)}
          onKeyDown={(e) => e.key === "Enter" && void submit()}
        />
      </label>
      <div className="actions">
        <button onClick={close}>Cancel</button>
        <button className="primary" disabled={!name.trim()} onClick={() => void submit()}>
          Place…
        </button>
      </div>
    </Modal>
  );
}

function AboutDialog() {
  const [info, setInfo] = useState<CoreInfo | null>(null);
  useEffect(() => {
    void api.coreInfo().then(setInfo);
  }, []);
  return (
    <Modal title="About ButureCAD" onClose={close}>
      <p>
        <strong>{info ? `${info.name} ${info.version}` : "ButureCAD"}</strong> — local-first
        electronic design automation.
      </p>
      <p className="muted">
        Everything runs on your machine: no account, no cloud, no telemetry. Proprietary software, ©
        2026 Pedro Buture.
      </p>
      <div className="actions">
        <button className="primary" onClick={close}>
          Close
        </button>
      </div>
    </Modal>
  );
}

export function Dialogs() {
  const dialog = useUi((s) => s.dialog);
  if (!dialog) return null;
  switch (dialog.kind) {
    case "new-project":
    case "save-as":
      return <FolderAndName mode={dialog.kind} />;
    case "rename-project":
      return <RenameDialog />;
    case "unsaved":
      return <UnsavedDialog title={dialog.title} then={dialog.then} />;
    case "recovery":
      return <RecoveryDialog />;
    case "about":
      return <AboutDialog />;
    case "symbol-picker":
      return <SymbolPicker power={dialog.power} />;
    case "label-name":
      return <LabelNameDialog global={dialog.global} />;
  }
}
