import { useEffect, useState } from "react";
import { api } from "../api";
import { call } from "../actions";
import type { LayerInfo } from "../bindings/LayerInfo";
import type { Snapshot } from "../bindings/Snapshot";
import { useUi } from "../store";
import { MODE_TITLES } from "../tools";

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
          onChange={(e) => void call(api.setPcbLayerVisible(layer.id, e.target.checked))}
        />
        <span className="swatch" style={{ background: layer.color }} />
        <span className="layer-name">{layer.name}</span>
      </label>
      {mode === "pcb" && (
        <button
          className="link"
          disabled={layer.active}
          title="Make this the active layer (routing tools will use it)"
          onClick={() => void call(api.setActiveLayer(layer.id))}
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
                · {sel[0].kind} #{sel[0].id}
              </span>
            </div>
            <dl className="props">
              {sel[0].properties.map((p) => (
                <div key={p.key} className="prop-row">
                  <dt>{p.label}</dt>
                  <dd title={p.value}>
                    {p.editable ? (
                      <PropertyInput objectId={sel[0].id} propKey={p.key} value={p.value} />
                    ) : (
                      p.value
                    )}
                  </dd>
                </div>
              ))}
            </dl>
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
      {snap.layers.length > 0 && (
        <section>
          <h3>{snap.mode === "pcb" ? "Layers" : "Object classes"}</h3>
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
