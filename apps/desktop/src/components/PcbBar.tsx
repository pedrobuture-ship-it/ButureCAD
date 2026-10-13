import { api } from "../api";
import { call } from "../actions";
import { store, useUi } from "../store";

/** Floating bar above the PCB canvas: stackup and board problems. */
export function PcbBar() {
  const info = useUi((s) => s.snapshot?.pcb);
  if (!info) return null;
  return (
    <div className="symbol-bar" role="toolbar" aria-label="PCB">
      <label>
        Copper layers
        <select
          value={info.copper_layers}
          aria-label="Copper layers"
          onChange={(e) => void call(api.pcbCommand("set_stackup", e.target.value))}
        >
          {[2, 4, 6, 8, 10, 12, 14, 16].map((n) => (
            <option key={n} value={n}>
              {n}
            </option>
          ))}
        </select>
      </label>
      <button
        title="Add, update and remove footprints and nets so the board matches the schematic (F8)"
        onClick={() => store.set({ dialog: { kind: "pcb-update" } })}
      >
        Update from schematic…
      </button>
      <label title="Show or hide the flying wires of unrouted connections (Shift+R)">
        <input
          type="checkbox"
          checked={info.ratsnest_visible}
          onChange={() => void call(api.pcbCommand("toggle_ratsnest"))}
        />
        Ratsnest ({info.unrouted})
      </label>
      <span className="muted">Active layer: {info.active_layer}</span>
      <span className="muted">
        {info.footprints} footprint{info.footprints === 1 ? "" : "s"} · {info.nets} net
        {info.nets === 1 ? "" : "s"}
      </span>
      {info.warnings.map((w) => (
        <span key={w} className="warn" role="status">
          {w}
        </span>
      ))}
    </div>
  );
}
