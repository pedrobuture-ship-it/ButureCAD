import { useEffect, useState } from "react";
import { api } from "../api";
import { call, toast } from "../actions";
import type { FootprintInfo } from "../bindings/FootprintInfo";
import type { LibraryInfo } from "../bindings/LibraryInfo";
import { store, useUi } from "../store";
import { FieldEditor } from "./SymbolEditor";

/** Footprint layers in the order the core numbers them (`LayerInfo.id`). */
export const FP_LAYER_IDS = ["silkscreen", "fabrication", "courtyard", "mask", "paste"] as const;

/** Inspector sections for the footprint being edited. */
export function FootprintFields() {
  const info = useUi((s) => s.snapshot?.footprint_editor);
  const [modelPath, setModelPath] = useState("");
  if (!info) return null;
  const set = (key: string) => (value: string) => void call(api.footprintEditorSet(key, value));
  return (
    <>
      <section>
        <h3>Footprint</h3>
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
            <dt>Pads</dt>
            <dd>{info.pad_count}</dd>
          </div>
        </dl>
        {info.warnings.length > 0 && (
          <ul className="warnings" aria-label="Footprint warnings">
            {info.warnings.map((w) => (
              <li key={w}>{w}</li>
            ))}
          </ul>
        )}
      </section>
      <section>
        <h3>Next pad</h3>
        <dl className="props">
          {info.pad_template.map((f) => (
            <div key={f.key} className="prop-row">
              <dt>{f.label}</dt>
              <dd>
                <FieldEditor
                  prop={f}
                  onCommit={(v) =>
                    void call(api.footprintEditorCommand("pad_template", `${f.key}=${v}`))
                  }
                />
              </dd>
            </div>
          ))}
        </dl>
        <p className="muted">
          Used by the pad tools. Through-hole pads get a drill that leaves a copper ring; the
          non-plated hole uses the drill as its size.
        </p>
      </section>
      <section>
        <h3>3D models</h3>
        <p className="muted">
          References are stored with the footprint. Nothing renders them yet (roadmap stages 27–28).
        </p>
        {info.models.map((m) => (
          <div key={m.index} className="model-ref">
            <dl className="props">
              {m.fields.map((f) => (
                <div key={f.key} className="prop-row">
                  <dt>{f.label}</dt>
                  <dd>
                    <FieldEditor prop={f} onCommit={set(f.key)} />
                  </dd>
                </div>
              ))}
            </dl>
            <button
              className="link"
              onClick={() => void call(api.footprintEditorCommand("remove_model", String(m.index)))}
            >
              Remove reference
            </button>
          </div>
        ))}
        <div className="row">
          <input
            className="prop-input"
            placeholder="models/part.step"
            value={modelPath}
            onChange={(e) => setModelPath(e.target.value)}
          />
          <button
            disabled={!modelPath.trim()}
            onClick={() => {
              void call(api.footprintEditorCommand("add_model", modelPath.trim()));
              setModelPath("");
            }}
          >
            Add
          </button>
        </div>
      </section>
    </>
  );
}

/** Floating bar above the footprint canvas. */
export function FootprintEditorBar() {
  const info = useUi((s) => s.snapshot?.footprint_editor);
  if (!info) return null;
  const cmd = (name: string, arg?: string) => void call(api.footprintEditorCommand(name, arg));
  return (
    <div className="symbol-bar" role="toolbar" aria-label="Footprint Editor">
      <strong title={info.name}>{info.name}</strong>
      <label>
        Draw on
        <select
          value={info.layer}
          onChange={(e) => cmd("layer", e.target.value)}
          aria-label="Layer for new graphics"
        >
          <option value="silkscreen">Silkscreen</option>
          <option value="fabrication">Fabrication</option>
          <option value="courtyard">Courtyard</option>
          <option value="mask">Solder mask</option>
          <option value="paste">Solder paste</option>
        </select>
      </label>
      <button
        title="Replace the courtyard with a rectangle around all pads (0.25 mm margin)"
        onClick={() => cmd("auto_courtyard")}
      >
        Courtyard from pads
      </button>
      <span className="spacer" />
      <button onClick={() => store.set({ dialog: { kind: "footprint-generate" } })}>
        Generate…
      </button>
      <button onClick={() => store.set({ dialog: { kind: "footprint-save" } })}>
        Save to library…
      </button>
      <button onClick={() => store.set({ dialog: { kind: "footprint-open" } })}>Open…</button>
      <button onClick={() => store.set({ dialog: { kind: "footprint-new" } })}>New…</button>
    </div>
  );
}

/** Shown in the Footprint Editor tab while no footprint is open. */
export function FootprintEditorHome() {
  const [libs, setLibs] = useState<LibraryInfo[]>([]);
  const [items, setItems] = useState<FootprintInfo[]>([]);
  useEffect(() => {
    api
      .footprintLibraries()
      .then(setLibs)
      .catch((e) => toast(String(e)));
    api
      .libraryFootprints()
      .then(setItems)
      .catch((e) => toast(String(e)));
  }, []);
  return (
    <div className="start">
      <h1>Footprint Editor</h1>
      <p>
        Place SMD, through-hole and non-plated pads (circle, rectangle, oval, rounded rectangle or
        custom), draw silkscreen, fabrication, courtyard, mask and paste graphics, add reference and
        value text, and generate DIP, SOIC, QFN, BGA and header packages.
      </p>
      <div className="row">
        <button
          className="primary"
          onClick={() => store.set({ dialog: { kind: "footprint-generate" } })}
        >
          Generate a package…
        </button>
        <button onClick={() => store.set({ dialog: { kind: "footprint-new" } })}>
          New footprint…
        </button>
        <button onClick={() => store.set({ dialog: { kind: "footprint-open" } })}>
          Open a footprint…
        </button>
      </div>
      <h3>Libraries</h3>
      <ul className="lib-list">
        {libs.length === 0 && <li className="muted">No footprint libraries yet.</li>}
        {libs.map((l) => (
          <li key={`${l.scope}:${l.name}`} title={l.error ?? ""}>
            <strong>{l.name}</strong>{" "}
            <span className="muted">
              {l.scope} · {l.error ? "unreadable" : `${l.item_count} footprints`}
            </span>
            {l.error && <span className="warn"> {l.error}</span>}
          </li>
        ))}
      </ul>
      <p className="muted">
        {items.length} footprints available. Project libraries live in the project&apos;s{" "}
        <code>libraries/</code> folder.
      </p>
    </div>
  );
}
