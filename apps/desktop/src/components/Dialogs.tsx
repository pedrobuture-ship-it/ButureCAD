import { useEffect, useState, type ReactNode } from "react";
import { api } from "../api";
import { call, pickFolder, toast } from "../actions";
import type { FootprintInfo } from "../bindings/FootprintInfo";
import type { UpdatePreview } from "../bindings/UpdatePreview";
import type { GeneratorInfo } from "../bindings/GeneratorInfo";
import type { LibraryInfo } from "../bindings/LibraryInfo";
import type { SymbolInfo } from "../bindings/SymbolInfo";
import type { CoreInfo } from "../bindings/CoreInfo";
import type { NetClassInfo } from "../bindings/NetClassInfo";
import type { PairInfo } from "../bindings/PairInfo";
import type { RulesInfo } from "../bindings/RulesInfo";
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

function FootprintNewDialog() {
  const [library, setLibrary] = useState("User");
  const [name, setName] = useState("NewFootprint");
  const valid = library.trim() !== "" && name.trim() !== "" && !/[:/\\]/.test(library + name);
  const submit = async () => {
    if (!valid) return;
    close();
    await call(api.footprintEditorCommand("new", `${library.trim()}:${name.trim()}`));
  };
  return (
    <Modal title="New footprint" onClose={close}>
      <label className="field">
        <span>Library name (the footprint is saved to this library later)</span>
        <input autoFocus value={library} onChange={(e) => setLibrary(e.target.value)} />
      </label>
      <label className="field">
        <span>Footprint name</span>
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

function FootprintOpenDialog() {
  const [items, setItems] = useState<FootprintInfo[]>([]);
  const [query, setQuery] = useState("");
  const [index, setIndex] = useState(0);
  useEffect(() => {
    api
      .libraryFootprints()
      .then(setItems)
      .catch((e) => toast(String(e)));
  }, []);
  const q = query.trim().toLowerCase();
  const shown = items.filter(
    (f) => q === "" || `${f.lib_id} ${f.description} ${f.tags}`.toLowerCase().includes(q),
  );
  const pick = async (f: FootprintInfo | undefined) => {
    if (!f) return;
    close();
    await call(api.footprintEditorCommand("open", f.lib_id));
  };
  return (
    <Modal title="Open a footprint" onClose={close}>
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
        {shown.length === 0 && <li className="muted">No matching footprint</li>}
        {shown.map((f, i) => (
          <li
            key={f.lib_id}
            role="option"
            aria-selected={i === index}
            className={i === index ? "sel" : ""}
            onMouseEnter={() => setIndex(i)}
            onClick={() => void pick(f)}
          >
            <strong>{f.lib_id}</strong>
            <span className="muted">
              {f.source} · {f.pad_count} pad{f.pad_count === 1 ? "" : "s"}
            </span>
          </li>
        ))}
      </ul>
      <p className="muted">
        Opening a library footprint copies it into the project; edits stay in the project until you
        save them back to a library.
      </p>
    </Modal>
  );
}

function FootprintSaveDialog() {
  const current = store.getState().snapshot?.footprint_editor?.name ?? "";
  const [scope, setScope] = useState("project");
  const [library, setLibrary] = useState(
    current.includes(":") ? (current.split(":")[0] ?? "User") : "User",
  );
  const [libs, setLibs] = useState<LibraryInfo[]>([]);
  useEffect(() => {
    api
      .footprintLibraries()
      .then(setLibs)
      .catch((e) => toast(String(e)));
  }, []);
  const valid = library.trim() !== "" && !/[:/\\]/.test(library);
  const submit = async () => {
    if (!valid) return;
    close();
    await call(api.footprintEditorCommand("save_to_library", `${scope}:${library.trim()}`));
  };
  return (
    <Modal title="Save footprint to library" onClose={close}>
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
          list="fp-save-libs"
          value={library}
          onChange={(e) => setLibrary(e.target.value)}
          onKeyDown={(e) => e.key === "Enter" && void submit()}
        />
        <datalist id="fp-save-libs">
          {libs
            .filter((l) => l.scope === scope)
            .map((l) => (
              <option key={l.name} value={l.name} />
            ))}
        </datalist>
      </label>
      <p className="muted">
        A footprint with the same name in that library is replaced. If the library differs from the
        footprint&apos;s current one, the footprint is renamed in this project too.
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

function FootprintGenerateDialog() {
  const [specs, setSpecs] = useState<GeneratorInfo[]>([]);
  const [family, setFamily] = useState("soic");
  const [values, setValues] = useState<Record<string, string>>({});
  useEffect(() => {
    api
      .footprintGenerators()
      .then(setSpecs)
      .catch((e) => toast(String(e)));
  }, []);
  const spec = specs.find((s) => s.family === family);
  const value = (key: string, def: string) => values[`${family}.${key}`] ?? def;
  const submit = async () => {
    if (!spec) return;
    const args = spec.params.map((p) => `${p.key}=${value(p.key, p.default)}`).join("|");
    close();
    await call(api.footprintEditorCommand("generate", `${family}|${args}`));
  };
  return (
    <Modal title="Generate a footprint" onClose={close}>
      <label className="field">
        <span>Package</span>
        <select
          autoFocus
          value={family}
          onChange={(e) => setFamily(e.target.value)}
          aria-label="Package family"
        >
          {specs.map((s) => (
            <option key={s.family} value={s.family}>
              {s.title}
            </option>
          ))}
        </select>
      </label>
      {spec?.params.map((p) =>
        p.kind === "flag" ? (
          <label key={p.key} className="field">
            <span>{p.label}</span>
            <select
              value={value(p.key, p.default)}
              onChange={(e) => setValues({ ...values, [`${family}.${p.key}`]: e.target.value })}
            >
              <option value="false">No</option>
              <option value="true">Yes</option>
            </select>
          </label>
        ) : (
          <label key={p.key} className="field">
            <span>{p.label}</span>
            <input
              value={value(p.key, p.default)}
              onChange={(e) => setValues({ ...values, [`${family}.${p.key}`]: e.target.value })}
              onKeyDown={(e) => e.key === "Enter" && void submit()}
            />
          </label>
        ),
      )}
      <p className="muted">
        The generator creates real pads, silkscreen, fabrication and courtyard geometry from the
        nominal package dimensions. Check them against the part&apos;s datasheet before use.
      </p>
      <div className="actions">
        <button onClick={close}>Cancel</button>
        <button className="primary" disabled={!spec} onClick={() => void submit()}>
          Generate
        </button>
      </div>
    </Modal>
  );
}

function FootprintTextDialog() {
  const [text, setText] = useState("TEXT");
  const submit = async () => {
    if (!text.trim()) return;
    close();
    await call(api.editorCommand(`tool:text:${text.trim()}`));
  };
  return (
    <Modal title="Text" onClose={close}>
      <label className="field">
        <span>Text to place (on the layer chosen in the bar)</span>
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

const PLAN_LABELS: Record<string, string> = {
  add_footprint: "Add",
  remove_footprint: "Remove",
  change_footprint: "Change footprint",
  link: "Link",
  update_field: "Update",
  add_net: "Add net",
  remove_net: "Remove net",
  rename_net: "Rename net",
  pad_net: "Pad net",
  warning: "Warning",
};

function PcbUpdateDialog() {
  const [preview, setPreview] = useState<UpdatePreview | null>(null);
  const [error, setError] = useState<string | null>(null);
  useEffect(() => {
    api
      .pcbUpdatePreview()
      .then(setPreview)
      .catch((e) => setError(String(e)));
  }, []);
  const apply = async () => {
    close();
    await call(api.pcbUpdateApply());
  };
  return (
    <Modal title="Update PCB from Schematic" onClose={close}>
      {error && <p className="warn">{error}</p>}
      {!preview && !error && <p className="muted">Comparing the schematic and the board…</p>}
      {preview && preview.lines.length === 0 && (
        <p>The board already matches the schematic. Nothing to do.</p>
      )}
      {preview && preview.lines.length > 0 && (
        <ul className="plan-list" aria-label="Planned changes">
          {preview.lines.map((l, i) => (
            <li key={i} className={`plan-${l.kind}`}>
              <span className="plan-kind">{PLAN_LABELS[l.kind] ?? l.kind}</span> {l.text}
            </li>
          ))}
        </ul>
      )}
      <p className="muted">
        Existing placement and routing are kept: footprints are updated in place, tracks stay where
        they are, and everything is one undo step. Nothing has been changed yet.
      </p>
      <div className="actions">
        <button onClick={close}>Cancel</button>
        <button className="primary" disabled={!preview?.can_apply} onClick={() => void apply()}>
          Apply changes
        </button>
      </div>
    </Modal>
  );
}

const CLASS_FIELDS: { key: keyof Omit<NetClassInfo, "name">; label: string }[] = [
  { key: "track_width", label: "Track width" },
  { key: "clearance", label: "Clearance" },
  { key: "via_diameter", label: "Via Ø" },
  { key: "via_drill", label: "Via drill" },
  { key: "diff_pair_width", label: "Pair width" },
  { key: "diff_pair_gap", label: "Pair gap" },
];
const LIMIT_FIELDS: { key: keyof RulesInfo; label: string }[] = [
  { key: "min_track_width", label: "Min. track width" },
  { key: "min_via_diameter", label: "Min. via Ø" },
  { key: "min_via_drill", label: "Min. via drill" },
  { key: "min_annular_ring", label: "Min. annular ring" },
  { key: "copper_to_edge", label: "Copper to edge" },
];

/** Net classes and board limits (mm). The router and DRC read the same values. */
function NetClassesDialog() {
  const [rules, setRules] = useState<RulesInfo | null>(null);
  const [newName, setNewName] = useState("");
  useEffect(() => {
    api
      .pcbRules()
      .then(setRules)
      .catch((e) => toast(String(e)));
  }, []);
  if (!rules)
    return (
      <Modal title="Net classes" onClose={close}>
        <p className="muted">Loading…</p>
      </Modal>
    );
  const setClass = (i: number, key: string, v: string) =>
    setRules({
      ...rules,
      classes: rules.classes.map((c, j) => (j === i ? { ...c, [key]: v } : c)),
    });
  const addClass = (name: string) => {
    const n = name.trim();
    if (!n || rules.classes.some((c) => c.name === n)) return;
    const base = rules.classes.find((c) => c.name === "Default") ?? rules.classes[0];
    const presets: Record<string, Partial<NetClassInfo>> = {
      Power: { track_width: "0.5", clearance: "0.25", via_diameter: "1", via_drill: "0.5" },
      USB: {
        track_width: "0.2",
        clearance: "0.2",
        via_diameter: "0.6",
        via_drill: "0.3",
        diff_pair_gap: "0.15",
      },
      Differential: { track_width: "0.2", via_diameter: "0.6", via_drill: "0.3" },
      HighCurrent: { track_width: "1", clearance: "0.3", via_diameter: "1.2", via_drill: "0.6" },
    };
    setRules({ ...rules, classes: [...rules.classes, { ...base, ...presets[n], name: n }] });
    setNewName("");
  };
  const setPair = (i: number, patch: Partial<PairInfo>) =>
    setRules({
      ...rules,
      pairs: rules.pairs.map((p, j) => (j === i ? { ...p, ...patch } : p)),
    });
  const removePair = (i: number) =>
    setRules({ ...rules, pairs: rules.pairs.filter((_, j) => j !== i) });
  const addPair = (p: PairInfo) =>
    setRules({
      ...rules,
      pairs: [...rules.pairs, p],
      suggested_pairs: rules.suggested_pairs.filter((s) => s.name !== p.name),
    });
  const removeClass = (name: string) =>
    setRules({
      ...rules,
      classes: rules.classes.filter((c) => c.name !== name),
      nets: rules.nets.map((n) => (n.class === name ? { ...n, class: "Default" } : n)),
    });
  const apply = async () => {
    close();
    await call(api.pcbRulesApply(rules));
  };
  const missingPresets = rules.presets.filter((p) => !rules.classes.some((c) => c.name === p));
  return (
    <Modal title="Net classes and design rules" onClose={close}>
      <table className="rules-table">
        <thead>
          <tr>
            <th>Class</th>
            {CLASS_FIELDS.map((f) => (
              <th key={f.key}>{f.label}</th>
            ))}
            <th />
          </tr>
        </thead>
        <tbody>
          {rules.classes.map((c, i) => (
            <tr key={c.name}>
              <td>
                <strong>{c.name}</strong>
              </td>
              {CLASS_FIELDS.map((f) => (
                <td key={f.key}>
                  <input
                    aria-label={`${c.name} ${f.label}`}
                    value={c[f.key]}
                    onChange={(e) => setClass(i, f.key, e.target.value)}
                  />
                </td>
              ))}
              <td>
                {c.name !== "Default" && (
                  <button className="link" onClick={() => removeClass(c.name)}>
                    Remove
                  </button>
                )}
              </td>
            </tr>
          ))}
        </tbody>
      </table>
      <div className="actions">
        {missingPresets.map((p) => (
          <button key={p} onClick={() => addClass(p)}>
            + {p}
          </button>
        ))}
        <input
          value={newName}
          placeholder="New class name"
          onChange={(e) => setNewName(e.target.value)}
          onKeyDown={(e) => e.key === "Enter" && addClass(newName)}
        />
        <button onClick={() => addClass(newName)}>Add</button>
      </div>
      <h3>Nets</h3>
      <div className="rules-nets">
        {rules.nets.length === 0 && <p className="muted">The board has no nets yet.</p>}
        {rules.nets.map((n) => (
          <label key={n.id}>
            {n.name}
            <select
              value={n.class}
              onChange={(e) =>
                setRules({
                  ...rules,
                  nets: rules.nets.map((m) =>
                    m.id === n.id ? { ...m, class: e.target.value } : m,
                  ),
                })
              }
            >
              {rules.classes.map((c) => (
                <option key={c.name}>{c.name}</option>
              ))}
            </select>
          </label>
        ))}
      </div>
      <h3>Differential pairs</h3>
      <table className="rules-table">
        <thead>
          <tr>
            <th>Pair</th>
            <th>+ net</th>
            <th>− net</th>
            <th>Width</th>
            <th>Gap</th>
            <th>Max skew</th>
            <th />
          </tr>
        </thead>
        <tbody>
          {rules.pairs.map((p, i) => (
            <tr key={p.name}>
              <td>
                <strong>{p.name}</strong>
              </td>
              {(["positive", "negative"] as const).map((side) => (
                <td key={side}>
                  <select
                    aria-label={`${p.name} ${side} net`}
                    value={p[side]}
                    onChange={(e) => setPair(i, { [side]: Number(e.target.value) })}
                  >
                    {rules.nets.map((n) => (
                      <option key={n.id} value={n.id}>
                        {n.name}
                      </option>
                    ))}
                  </select>
                </td>
              ))}
              {(["width", "gap", "max_skew"] as const).map((k) => (
                <td key={k}>
                  <input
                    aria-label={`${p.name} ${k}`}
                    value={p[k]}
                    placeholder={k === "max_skew" ? "" : "class"}
                    onChange={(e) => setPair(i, { [k]: e.target.value })}
                  />
                </td>
              ))}
              <td>
                <button className="link" onClick={() => removePair(i)}>
                  Remove
                </button>
              </td>
            </tr>
          ))}
        </tbody>
      </table>
      <div className="actions">
        {rules.suggested_pairs.map((p) => (
          <button
            key={p.name}
            title="Add the pair found from the net names"
            onClick={() => addPair(p)}
          >
            + {p.name}
          </button>
        ))}
        {rules.suggested_pairs.length === 0 && (
          <span className="muted">
            Nets named like USB_D+ / USB_D-, X_P / X_N are offered here to add as pairs.
          </span>
        )}
      </div>
      <h3>Board limits (DRC)</h3>
      <div className="rules-limits">
        {LIMIT_FIELDS.map((f) => (
          <label key={f.key}>
            {f.label}
            <input
              value={rules[f.key] as string}
              onChange={(e) => setRules({ ...rules, [f.key]: e.target.value })}
            />
          </label>
        ))}
      </div>
      <p className="muted">
        All values in mm. The router uses a net's class for track width and via size (unless you
        type a width in the PCB bar; clear it to return to “auto”). DRC enforces the larger
        clearance of the two nets involved. Differential pair values are stored for the pair router
        (roadmap 23).
      </p>
      <div className="actions">
        <button onClick={close}>Cancel</button>
        <button className="primary" onClick={() => void apply()}>
          Apply
        </button>
      </div>
    </Modal>
  );
}

function PcbPlaceDialog() {
  const [items, setItems] = useState<FootprintInfo[]>([]);
  const [query, setQuery] = useState("");
  const [index, setIndex] = useState(0);
  useEffect(() => {
    api
      .pcbFootprints()
      .then(setItems)
      .catch((e) => toast(String(e)));
  }, []);
  const q = query.trim().toLowerCase();
  const shown = items.filter(
    (f) => q === "" || `${f.lib_id} ${f.description} ${f.tags}`.toLowerCase().includes(q),
  );
  const pick = async (f: FootprintInfo | undefined) => {
    if (!f) return;
    close();
    await call(api.editorCommand(`tool:place_footprint:${f.lib_id}`));
  };
  return (
    <Modal title="Place a footprint" onClose={close}>
      <input
        autoFocus
        value={query}
        placeholder="Search the project and the libraries…"
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
        {shown.length === 0 && (
          <li className="muted">
            No footprints yet — generate or draw one in the Footprint Editor first.
          </li>
        )}
        {shown.map((f, i) => (
          <li
            key={f.lib_id}
            role="option"
            aria-selected={i === index}
            className={i === index ? "sel" : ""}
            onMouseEnter={() => setIndex(i)}
            onClick={() => void pick(f)}
          >
            <strong>{f.lib_id}</strong>
            <span className="muted">
              {f.source} · {f.pad_count} pads
            </span>
          </li>
        ))}
      </ul>
      <p className="muted">
        Click on the board to place. R rotates, F flips to the back side, Esc stops.
      </p>
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
    case "pcb-update":
      return <PcbUpdateDialog />;
    case "pcb-place":
      return <PcbPlaceDialog />;
    case "net-classes":
      return <NetClassesDialog />;
    case "footprint-new":
      return <FootprintNewDialog />;
    case "footprint-open":
      return <FootprintOpenDialog />;
    case "footprint-save":
      return <FootprintSaveDialog />;
    case "footprint-generate":
      return <FootprintGenerateDialog />;
    case "footprint-text":
      return <FootprintTextDialog />;
    case "erc-waive":
      return <ErcWaiveDialog fingerprint={dialog.fingerprint} message={dialog.message} />;
  }
}
