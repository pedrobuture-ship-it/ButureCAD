import { useEffect, useState } from "react";
import { api } from "../api";
import { call, toast } from "../actions";
import type { PropertyInfo } from "../bindings/PropertyInfo";
import type { LibraryInfo } from "../bindings/LibraryInfo";
import type { SymbolInfo } from "../bindings/SymbolInfo";
import { store, useUi } from "../store";

const unitLetter = (u: number) => String.fromCharCode(64 + u);

/** A symbol-level or pin-template field: drop-down for enumerations, text otherwise. */
export function FieldEditor({
  prop,
  onCommit,
}: {
  prop: PropertyInfo;
  onCommit: (value: string) => void;
}) {
  const [text, setText] = useState(prop.value);
  useEffect(() => setText(prop.value), [prop.value, prop.key]);
  if (prop.options.length > 0) {
    return (
      <select
        className="prop-input"
        aria-label={prop.label}
        value={prop.value}
        disabled={!prop.editable}
        onChange={(e) => onCommit(e.target.value)}
      >
        {prop.options.map((o) => (
          <option key={o.value} value={o.value}>
            {o.label}
          </option>
        ))}
      </select>
    );
  }
  return (
    <input
      className="prop-input"
      aria-label={prop.label}
      value={text}
      readOnly={!prop.editable}
      onChange={(e) => setText(e.target.value)}
      onBlur={() => text !== prop.value && onCommit(text)}
      onKeyDown={(e) => {
        if (e.key === "Enter") e.currentTarget.blur();
        else if (e.key === "Escape") setText(prop.value);
      }}
    />
  );
}

/** Inspector sections for the symbol being edited. */
export function SymbolFields() {
  const info = useUi((s) => s.snapshot?.symbol_editor);
  const [newKey, setNewKey] = useState("");
  if (!info) return null;
  const set = (key: string) => (value: string) => void call(api.symbolEditorSet(key, value));
  return (
    <>
      <section>
        <h3>Symbol</h3>
        <dl className="props">
          {info.fields.map((f) => (
            <div key={f.key} className="prop-row">
              <dt>{f.label}</dt>
              <dd>
                <FieldEditor prop={f} onCommit={set(f.key)} />
              </dd>
            </div>
          ))}
          <div className="prop-row">
            <dt>Used by</dt>
            <dd>{info.used_by} placed object(s)</dd>
          </div>
        </dl>
        {info.warnings.length > 0 && (
          <ul className="warnings" aria-label="Symbol warnings">
            {info.warnings.map((w) => (
              <li key={w}>{w}</li>
            ))}
          </ul>
        )}
      </section>
      <section>
        <h3>Custom properties</h3>
        <dl className="props">
          {info.custom.map((f) => (
            <div key={f.key} className="prop-row">
              <dt>{f.label}</dt>
              <dd>
                <FieldEditor prop={f} onCommit={set(f.key)} />
              </dd>
            </div>
          ))}
        </dl>
        <div className="row">
          <input
            className="prop-input"
            placeholder="New property name"
            value={newKey}
            onChange={(e) => setNewKey(e.target.value)}
          />
          <button
            disabled={!newKey.trim()}
            onClick={() => {
              void call(api.symbolEditorSet(`property:${newKey.trim()}`, "value"));
              setNewKey("");
            }}
          >
            Add
          </button>
        </div>
      </section>
      <section>
        <h3>Next pin</h3>
        <dl className="props">
          {info.pin_template.map((f) => (
            <div key={f.key} className="prop-row">
              <dt>{f.label}</dt>
              <dd>
                <FieldEditor
                  prop={f}
                  onCommit={(v) =>
                    void call(api.symbolEditorCommand("pin_template", `${f.key}=${v}`))
                  }
                />
              </dd>
            </div>
          ))}
        </dl>
        <p className="muted">Used by the Pin tool; R rotates it while the tool is active.</p>
      </section>
    </>
  );
}

/** Floating bar above the symbol canvas: units, representations, library actions. */
export function SymbolEditorBar() {
  const info = useUi((s) => s.snapshot?.symbol_editor);
  if (!info) return null;
  const cmd = (name: string, arg?: string) => void call(api.symbolEditorCommand(name, arg));
  const units = Array.from({ length: info.unit_count }, (_, i) => i + 1);
  return (
    <div className="symbol-bar" role="toolbar" aria-label="Symbol Editor">
      <strong title={info.lib_id}>{info.lib_id}</strong>
      <label>
        Unit
        <select
          value={info.unit}
          onChange={(e) => cmd("unit", e.target.value)}
          aria-label="Unit shown"
        >
          {units.map((u) => (
            <option key={u} value={u}>
              {unitLetter(u)}
            </option>
          ))}
        </select>
      </label>
      <button title="Add an empty unit" onClick={() => cmd("add_unit")}>
        + Unit
      </button>
      <button
        title="Add a unit that starts as a copy of the unit shown (pins get new numbers)"
        onClick={async () => {
          if (await call(api.symbolEditorCommand("add_unit"))) {
            const next =
              store.getState().snapshot?.symbol_editor?.unit_count ?? info.unit_count + 1;
            await call(api.symbolEditorCommand("copy_unit", `${info.unit}:${next}`));
          }
        }}
      >
        Duplicate unit
      </button>
      <button
        title="Remove the last unit (it must be empty)"
        disabled={info.unit_count <= 1}
        onClick={() => cmd("remove_unit")}
      >
        − Unit
      </button>
      <label>
        Shape
        <select
          value={info.representation}
          onChange={(e) => cmd("repr", e.target.value)}
          aria-label="Representation shown"
        >
          <option value={1}>Normal</option>
          <option value={2} disabled={!info.has_alternate}>
            Alternate
          </option>
        </select>
      </label>
      {info.has_alternate ? (
        <button onClick={() => cmd("alternate_remove")}>Remove alternate</button>
      ) : (
        <button
          title="Create an alternate (De Morgan style) representation starting from a copy of the normal one"
          onClick={() => cmd("alternate_create")}
        >
          Create alternate
        </button>
      )}
      <span className="spacer" />
      <button onClick={() => store.set({ dialog: { kind: "symbol-save" } })}>
        Save to library…
      </button>
      <button onClick={() => store.set({ dialog: { kind: "symbol-open" } })}>Open…</button>
      <button onClick={() => store.set({ dialog: { kind: "symbol-new" } })}>New…</button>
    </div>
  );
}

/** Shown in the Symbol Editor tab while no symbol is open. */
export function SymbolEditorHome() {
  const [libs, setLibs] = useState<LibraryInfo[]>([]);
  const [symbols, setSymbols] = useState<SymbolInfo[]>([]);
  useEffect(() => {
    api
      .symbolLibraries()
      .then(setLibs)
      .catch((e) => toast(String(e)));
    api
      .librarySymbols()
      .then(setSymbols)
      .catch((e) => toast(String(e)));
  }, []);
  return (
    <div className="start">
      <h1>Symbol Editor</h1>
      <p>
        Draw pins, lines, rectangles, circles, arcs and text; give the symbol units, an alternate
        representation and metadata; save it to a library folder you own.
      </p>
      <div className="row">
        <button className="primary" onClick={() => store.set({ dialog: { kind: "symbol-new" } })}>
          New symbol…
        </button>
        <button onClick={() => store.set({ dialog: { kind: "symbol-open" } })}>
          Open a symbol…
        </button>
      </div>
      <h3>Libraries</h3>
      <ul className="lib-list">
        {libs.map((l) => (
          <li key={`${l.scope}:${l.name}`} title={l.error ?? ""}>
            <strong>{l.name}</strong>{" "}
            <span className="muted">
              {l.scope} · {l.error ? "unreadable" : `${l.item_count} symbols`}
            </span>
            {l.error && <span className="warn"> {l.error}</span>}
          </li>
        ))}
      </ul>
      <p className="muted">
        {symbols.length} symbols available. Project libraries live in the project&apos;s{" "}
        <code>libraries/</code> folder; per-user libraries in the application data folder.
      </p>
    </div>
  );
}
