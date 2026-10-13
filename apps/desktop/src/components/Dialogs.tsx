import { useEffect, useState, type ReactNode } from "react";
import { api } from "../api";
import { call, pickFolder, toast } from "../actions";
import type { LibraryInfo } from "../bindings/LibraryInfo";
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

function SymbolNewDialog() {
  const [library, setLibrary] = useState("User");
  const [name, setName] = useState("NewSymbol");
  const [libs, setLibs] = useState<LibraryInfo[]>([]);
  useEffect(() => {
    api
      .symbolLibraries()
      .then(setLibs)
      .catch((e) => toast(String(e)));
  }, []);
  const valid = library.trim() !== "" && name.trim() !== "" && !/[:/\\]/.test(library + name);
  const submit = async () => {
    if (!valid) return;
    close();
    await call(api.symbolEditorCommand("new", `${library.trim()}:${name.trim()}`));
  };
  return (
    <Modal title="New symbol" onClose={close}>
      <label className="field">
        <span>Library name (the symbol is saved to this library later)</span>
        <input
          autoFocus
          list="symbol-libs"
          value={library}
          onChange={(e) => setLibrary(e.target.value)}
        />
        <datalist id="symbol-libs">
          {libs
            .filter((l) => l.scope !== "builtin")
            .map((l) => (
              <option key={`${l.scope}${l.name}`} value={l.name} />
            ))}
        </datalist>
      </label>
      <label className="field">
        <span>Symbol name</span>
        <input
          value={name}
          onChange={(e) => setName(e.target.value)}
          onKeyDown={(e) => e.key === "Enter" && void submit()}
        />
      </label>
      <div className="actions">
        <button onClick={close}>Cancel</button>
        <button className="primary" disabled={!valid} onClick={() => void submit()}>
          Create
        </button>
      </div>
    </Modal>
  );
}

function SymbolOpenDialog() {
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
    (s) => q === "" || `${s.lib_id} ${s.description}`.toLowerCase().includes(q),
  );
  const pick = async (s: SymbolInfo | undefined) => {
    if (!s) return;
    close();
    await call(api.symbolEditorCommand("open", s.lib_id));
  };
  return (
    <Modal title="Open a symbol" onClose={close}>
      <input
        autoFocus
        value={query}
        placeholder="Search libraries and the project…"
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
            <strong>{s.lib_id}</strong>
            <span className="muted">
              {s.source} · {s.pin_count} pin{s.pin_count === 1 ? "" : "s"}
              {s.unit_count > 1 ? ` · ${s.unit_count} units` : ""}
              {s.has_alternate ? " · alt" : ""}
            </span>
          </li>
        ))}
      </ul>
      <p className="muted">
        Opening a library symbol copies it into the project; edits stay in the project until you
        save them back to a library.
      </p>
    </Modal>
  );
}

function SymbolSaveDialog() {
  const current = store.getState().snapshot?.symbol_editor?.lib_id ?? "";
  const [scope, setScope] = useState("project");
  const [library, setLibrary] = useState(
    current.includes(":") ? (current.split(":")[0] ?? "User") : "User",
  );
  const [libs, setLibs] = useState<LibraryInfo[]>([]);
  useEffect(() => {
    api
      .symbolLibraries()
      .then(setLibs)
      .catch((e) => toast(String(e)));
  }, []);
  const builtin = library.trim() === "Buture";
  const valid = library.trim() !== "" && !builtin && !/[:/\\]/.test(library);
  const submit = async () => {
    if (!valid) return;
    close();
    await call(api.symbolEditorCommand("save_to_library", `${scope}:${library.trim()}`));
  };
  return (
    <Modal title="Save symbol to library" onClose={close}>
      <label className="field">
        <span>Where</span>
        <select value={scope} onChange={(e) => setScope(e.target.value)}>
          <option value="project">
            This project (libraries/ folder, travels with the project)
          </option>
          <option value="user">My user libraries (shared by all projects)</option>
        </select>
      </label>
      <label className="field">
        <span>Library</span>
        <input
          autoFocus
          list="save-libs"
          value={library}
          onChange={(e) => setLibrary(e.target.value)}
          onKeyDown={(e) => e.key === "Enter" && void submit()}
        />
        <datalist id="save-libs">
          {libs
            .filter((l) => l.scope === scope)
            .map((l) => (
              <option key={l.name} value={l.name} />
            ))}
        </datalist>
      </label>
      {builtin && <p className="warn">“Buture” is the read-only built-in library.</p>}
      <p className="muted">
        If the library already has a symbol with this name it is replaced. If the library name
        differs from the symbol&apos;s, the symbol is renamed in this project too (placed parts
        follow).
      </p>
      <div className="actions">
        <button onClick={close}>Cancel</button>
        <button className="primary" disabled={!valid} onClick={() => void submit()}>
          Save
        </button>
      </div>
    </Modal>
  );
}

function SymbolTextDialog() {
  const [text, setText] = useState("TEXT");
  const submit = async () => {
    if (!text.trim()) return;
    close();
    await call(api.editorCommand(`tool:text:${text.trim()}`));
  };
  return (
    <Modal title="Text" onClose={close}>
      <label className="field">
        <span>Text to place</span>
        <input
          autoFocus
          value={text}
          onChange={(e) => setText(e.target.value)}
          onKeyDown={(e) => e.key === "Enter" && void submit()}
        />
      </label>
      <div className="actions">
        <button onClick={close}>Cancel</button>
        <button className="primary" disabled={!text.trim()} onClick={() => void submit()}>
          Place…
        </button>
      </div>
    </Modal>
  );
}

function ErcWaiveDialog({ fingerprint, message }: { fingerprint: string; message: string }) {
  const [reason, setReason] = useState("");
  const submit = async () => {
    if (!reason.trim()) return;
    close();
    await call(api.ercWaive(fingerprint, reason.trim()));
  };
  return (
    <Modal title="Waive an ERC finding" onClose={close}>
      <p>{message}</p>
      <label className="field">
        <span>Why is this acceptable? (stored in the project; required)</span>
        <textarea autoFocus rows={3} value={reason} onChange={(e) => setReason(e.target.value)} />
      </label>
      <p className="muted">
        The finding stays in the list, marked as waived with this reason, and is not counted. If the
        circuit changes so that it no longer applies, the waiver is flagged as stale.
      </p>
      <div className="actions">
        <button onClick={close}>Cancel</button>
        <button className="primary" disabled={!reason.trim()} onClick={() => void submit()}>
          Waive
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
    case "symbol-new":
      return <SymbolNewDialog />;
    case "symbol-open":
      return <SymbolOpenDialog />;
    case "symbol-save":
      return <SymbolSaveDialog />;
    case "symbol-text":
      return <SymbolTextDialog />;
    case "erc-waive":
      return <ErcWaiveDialog fingerprint={dialog.fingerprint} message={dialog.message} />;
  }
}
