import { api } from "../api";
import { call } from "../actions";
import { useUi } from "../store";

const LAYERS = [
  ["substrate", "Substrate"],
  ["mask", "Solder mask"],
  ["copper", "Copper"],
  ["silkscreen", "Silkscreen"],
  ["components", "Components"],
] as const;

/** Controls of the 3D view. Rendering and the model live in Rust; this only sends actions. */
export function Viewer3dBar() {
  const info = useUi((s) => s.snapshot?.viewer3d);
  if (!info) return null;
  const cmd = (name: string, arg?: string) => void call(api.viewer3dCommand(name, arg));
  return (
    <div className="symbol-bar" role="toolbar" aria-label="3D view">
      <label>
        Projection
        <select
          aria-label="Projection"
          value={info.projection}
          onChange={(e) => cmd("projection", e.target.value)}
        >
          <option value="perspective">Perspective</option>
          <option value="orthographic">Orthographic</option>
        </select>
      </label>
      {(["top", "bottom", "front", "iso"] as const).map((v) => (
        <button key={v} onClick={() => cmd("view", v)}>
          {v[0].toUpperCase() + v.slice(1)}
        </button>
      ))}
      <button title="Frame the whole board" onClick={() => cmd("frame")}>
        Frame
      </button>
      {LAYERS.map(([k, label]) => (
        <label key={k}>
          <input
            type="checkbox"
            checked={info[k]}
            onChange={(e) => cmd("show", `${k}:${e.target.checked ? 1 : 0}`)}
          />
          {label}
        </label>
      ))}
      <span className="muted">
        Drag: orbit · Shift/right drag: pan · wheel: zoom · {info.components_count} component
        {info.components_count === 1 ? "" : "s"}
        {info.with_model_refs > 0
          ? ` (${info.with_model_refs} reference a 3D model: drawn as a tinted box, models are not parsed yet)`
          : " (drawn as boxes from the courtyard)"}
      </span>
      {!info.has_outline && (
        <span className="warn" role="status">
          There is no closed board outline: draw one on Edge.Cuts in the PCB tab.
        </span>
      )}
    </div>
  );
}
