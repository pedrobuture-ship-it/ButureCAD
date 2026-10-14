import { useEffect, useState } from "react";
import { api } from "../api";
import { call } from "../actions";
import { store, useUi } from "../store";

/** A text field that sends its value to the core on Enter/blur and shows the core's value. */
function CommitInput({
  label,
  value,
  onCommit,
  width = 56,
}: {
  label: string;
  value: string;
  onCommit: (v: string) => void;
  width?: number;
}) {
  const [text, setText] = useState(value);
  useEffect(() => setText(value), [value]);
  return (
    <label>
      {label}
      <input
        className="prop-input"
        style={{ width }}
        aria-label={label}
        value={text}
        onChange={(e) => setText(e.target.value)}
        onBlur={() => text !== value && onCommit(text)}
        onKeyDown={(e) => {
          if (e.key === "Enter") e.currentTarget.blur();
          else if (e.key === "Escape") setText(value);
          e.stopPropagation();
        }}
      />
    </label>
  );
}

/** Floating bar above the PCB canvas: stackup, routing options and board problems. */
export function PcbBar() {
  const info = useUi((s) => s.snapshot?.pcb);
  if (!info) return null;
  const cmd = (name: string, arg?: string) => void call(api.pcbCommand(name, arg));
  return (
    <div className="symbol-bar" role="toolbar" aria-label="PCB">
      <label>
        Copper layers
        <select
          value={info.copper_layers}
          aria-label="Copper layers"
          onChange={(e) => cmd("set_stackup", e.target.value)}
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
      <button
        title="Net classes: track width, clearance and via size per net, and the board limits"
        onClick={() => store.set({ dialog: { kind: "net-classes" } })}
      >
        Net classes…
      </button>
      <label title="Show or hide the flying wires of unrouted connections (Shift+R)">
        <input
          type="checkbox"
          checked={info.ratsnest_visible}
          onChange={() => cmd("toggle_ratsnest")}
        />
        Ratsnest ({info.unrouted})
      </label>
      <label title="Walk around: routes detour around other copper, keepouts and the board edge; Ignore: draws exactly what you click (DRC reports problems)">
        Obstacles
        <select
          value={info.route_obstacles}
          aria-label="Obstacle mode"
          onChange={(e) => cmd("route_obstacles", e.target.value)}
        >
          <option value="walk">Walk around</option>
          <option value="ignore">Ignore obstacles</option>
        </select>
      </label>
      <CommitInput
        label={info.route_width_auto ? `Width (mm) · ${info.route_class}` : "Track width (mm)"}
        value={info.route_width_mm}
        onCommit={(v) => cmd("route_width", v)}
      />
      <label>
        Angle
        <select
          value={info.route_mode}
          aria-label="Routing angle"
          onChange={(e) => cmd("route_mode", e.target.value)}
        >
          <option value="45">45°</option>
          <option value="90">90°</option>
          <option value="free">Free</option>
        </select>
      </label>
      <CommitInput
        label={info.via_size_auto ? "Via Ø/drill · class" : "Via Ø/drill (mm)"}
        value={info.via_size}
        width={84}
        onCommit={(v) => cmd("via_size", v)}
      />
      <span className="muted">Active layer: {info.active_layer}</span>
      <span className="muted">
        {info.footprints} footprint{info.footprints === 1 ? "" : "s"} · {info.nets} net
        {info.nets === 1 ? "" : "s"}
      </span>
      {info.route && (
        <span role="status" className="route-status">
          Routing {info.route.net} on {info.route.layer} · {info.route.length_mm} mm
          {info.route.vias > 0 ? ` · ${info.route.vias} via(s)` : ""}
        </span>
      )}
      {info.route?.problem && (
        <span className="warn" role="status">
          Cannot route there: {info.route.problem}
        </span>
      )}
      {info.warnings.map((w) => (
        <span key={w} className="warn" role="status">
          {w}
        </span>
      ))}
    </div>
  );
}
