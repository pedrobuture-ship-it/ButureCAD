import { useEffect, useState } from "react";
import { api } from "../api";
import { call } from "../actions";
import type { FootprintSuggestion } from "../bindings/FootprintSuggestion";
import type { LayerInfo } from "../bindings/LayerInfo";
import type { Snapshot } from "../bindings/Snapshot";
import { useUi } from "../store";
import { MODE_TITLES } from "../tools";
import { FootprintFields, FP_LAYER_IDS } from "./FootprintEditor";
import { FieldEditor, SymbolFields } from "./SymbolEditor";

/**
 * Footprints that fit the selected schematic part, like KiCad's footprint chooser: those matching
 * the symbol's filters first, then every footprint with a pad for each pin. Open by itself while
 * the part has no footprint yet; a click assigns it (one undoable command).
 */
function FootprintSuggestions({ objectId, current }: { objectId: number; current: string }) {
  const [open, setOpen] = useState(current === "");
  const [items, setItems] = useState<FootprintSuggestion[] | null>(null);
  const [error, setError] = useState("");
  // A newly selected part opens the list if it still needs a footprint.
  useEffect(() => setOpen(current === ""), [objectId, current === ""]);
  useEffect(() => {
    if (!open) return;
    let alive = true;
    setItems(null);
    setError("");
    api
      .footprintSuggestions(objectId)
      .then((r) => alive && setItems(r))
      .catch((e) => alive && setError(String(e)));
    return () => {
      alive = false;
    };
  }, [open, objectId]);
  return (
    <div className="fp-suggest">
      <button className="link" onClick={() => setOpen(!open)} aria-expanded={open}>
        {open ? "Hide footprint suggestions" : "Suggest footprints…"}
      </button>
      {open && error && <p className="muted">{error}</p>}
      {open && !error && items === null && <p className="muted">Looking…</p>}
      {open && items && items.length === 0 && (
        <p className="muted">
          No footprint in the libraries has a pad for every pin of this symbol.
        </p>
      )}
      {open && items && items.length > 0 && (
        <ul className="fp-suggest-list" aria-label="Suggested footprints">
          {items.map((f) => (
            <li key={f.lib_id}>
              <button
                className={f.lib_id === current ? "fp-item current" : "fp-item"}
                title={`${f.description}\n${f.reason_text}`}
                onClick={() => void call(api.propertySet(objectId, "footprint", f.lib_id))}
              >
                <span className="fp-name">{f.name}</span>
                <span className="muted">
                  {f.library} · {f.pad_count} pads{f.reason === "filter" ? " · ★" : ""}
                </span>
              </button>
            </li>
          ))}
        </ul>
      )}
    </div>
  );
}

/** An editable property: commits on Enter or blur, and only when the value changed. */
function PropertyInput({
  objectId,
  propKey,
  value,
}: {
  objectId: number;
  propKey: string;
  value: string;
}) {
  const [text, setText] = useState(value);
  useEffect(() => setText(value), [value, objectId]);
  const commit = () => {
    if (text !== value) void call(api.propertySet(objectId, propKey, text));
  };
  return (
    <input
      className="prop-input"
      value={text}
      aria-label={propKey}
      onChange={(e) => setText(e.target.value)}
      onBlur={commit}
      onKeyDown={(e) => {
        if (e.key === "Enter") e.currentTarget.blur();
        else if (e.key === "Escape") setText(value);
      }}
    />
  );
}

function LayerRow({ layer, mode }: { layer: LayerInfo; mode: Snapshot["mode"] }) {
  return (
    <li className={layer.active ? "layer active" : "layer"}>
      <label
        title={
          layer.toggleable ? "Show or hide this layer" : "Schematic layers cannot be hidden yet"
        }
      >
        <input
          type="checkbox"
          checked={layer.visible}
          disabled={!layer.toggleable}
          onChange={(e) =>
            void call(
              mode === "footprint_editor"
                ? api.footprintEditorCommand(
                    "layer_visible",
                    `${FP_LAYER_IDS[layer.id] ?? ""}:${e.target.checked ? 1 : 0}`,
                  )
                : api.setPcbLayerVisible(layer.id, e.target.checked),
            )
          }
        />
        <span className="swatch" style={{ background: layer.color }} />
        <span className="layer-name">{layer.name}</span>
      </label>
      {(mode === "pcb" || mode === "footprint_editor") && (
        <button
          className="link"
          disabled={layer.active}
          title={
            mode === "pcb"
              ? "Make this the active layer (routing tools will use it)"
              : "Draw new graphics on this layer"
          }
          onClick={() =>
            void call(
              mode === "footprint_editor"
                ? api.footprintEditorCommand("layer", FP_LAYER_IDS[layer.id])
                : api.setActiveLayer(layer.id),
            )
          }
        >
          {layer.active ? "active" : "set active"}
        </button>
      )}
    </li>
  );
}

export function Inspector() {
  const snap = useUi((s) => s.snapshot);
  if (!snap) return <aside className="inspector" />;
  const sel = snap.selection;
  return (
    <aside className="inspector" aria-label="Properties inspector">
      <section>
        <h3>Properties</h3>
        {!snap.project && <p className="muted">No project open.</p>}
        {snap.project && sel.length === 0 && (
          <dl className="props">
            <dt>Project</dt>
            <dd>{snap.project.name}</dd>
            <dt>Folder</dt>
            <dd className="path" title={snap.project.path}>
              {snap.project.path}
            </dd>
            <dt>View</dt>
            <dd>{MODE_TITLES[snap.mode]}</dd>
            <dt>Objects</dt>
            <dd>{snap.object_count}</dd>
            <dt>Status</dt>
            <dd>{snap.project.dirty ? "unsaved changes" : "saved"}</dd>
          </dl>
        )}
        {sel.length === 1 && sel[0] && (
          <>
            <div className="sel-title">
              <strong>{sel[0].title}</strong>
              <span className="muted">
                {" "}
                · {sel[0].kind}
                {snap.mode === "schematic" || snap.mode === "pcb" ? ` #${sel[0].id}` : ""}
              </span>
            </div>
            <dl className="props">
              {sel[0].properties.map((p) => (
                <div key={p.key} className="prop-row">
                  <dt>{p.label}</dt>
                  <dd title={p.value}>
                    {p.editable && p.options.length > 0 ? (
                      <FieldEditor
                        prop={p}
                        onCommit={(v) => void call(api.propertySet(sel[0]!.id, p.key, v))}
                      />
                    ) : p.editable ? (
                      <PropertyInput objectId={sel[0].id} propKey={p.key} value={p.value} />
                    ) : (
                      p.value
                    )}
                  </dd>
                </div>
              ))}
            </dl>
            {snap.mode === "schematic" && sel[0].kind === "component" && (
              <FootprintSuggestions
                objectId={sel[0].id}
                current={sel[0].properties.find((p) => p.key === "footprint")?.value ?? ""}
              />
            )}
          </>
        )}
        {sel.length > 1 && (
          <>
            <div className="sel-title">
              <strong>{sel.length} objects selected</strong>
            </div>
            <ul className="sel-list">
              {sel.slice(0, 50).map((s) => (
                <li key={s.id}>
                  {s.title} <span className="muted">{s.kind}</span>
                </li>
              ))}
              {sel.length > 50 && <li className="muted">…and {sel.length - 50} more</li>}
            </ul>
          </>
        )}
      </section>
      {snap.mode === "symbol_editor" && <SymbolFields />}
      {snap.mode === "footprint_editor" && <FootprintFields />}
      {snap.layers.length > 0 && (
        <section>
          <h3>
            {snap.mode === "pcb" || snap.mode === "footprint_editor" ? "Layers" : "Object classes"}
          </h3>
          <ul className="layers">
            {snap.layers.map((l) => (
              <LayerRow key={l.id} layer={l} mode={snap.mode} />
            ))}
          </ul>
        </section>
      )}
    </aside>
  );
}
