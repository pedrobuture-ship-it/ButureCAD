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
      <label title="Walk around: routes detour around other copper, keepouts and the board edge. Push & shove: existing tracks of other nets are pushed aside when that is valid, otherwise it walks around. Ignore: draws exactly what you click (DRC reports problems)">
        Obstacles
        <select
          value={info.route_obstacles}
          aria-label="Obstacle mode"
          onChange={(e) => cmd("route_obstacles", e.target.value)}
        >
          <option value="walk">Walk around</option>
          <option value="push">Push &amp; shove</option>
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
      {info.route?.note && !info.route.problem && (
        <span role="status" className="muted">
          {info.route.note}
        </span>
      )}
      {info.length && (
        <>
          <span
            role="status"
            className={
              info.length.target_mm === null
                ? "muted"
                : info.length.within
                  ? "route-status"
                  : "warn"
            }
            title="Routed track length of the net (vias and pad stubs inside footprints are not counted)"
          >
            Length {info.length.net}: {info.length.current_mm} mm
            {info.length.target_mm !== null && (
              <>
                {" "}
                · target {info.length.target_mm} ± {info.length.tolerance_mm}
                {info.length.from_pair ? " (partner leg)" : ""} · Δ {info.length.difference_mm}
              </>
            )}
            {info.length.predicted_mm !== null && <> → {info.length.predicted_mm} mm</>}
            {info.length.partial && " (partial: segment too short)"}
          </span>
          <CommitInput
            label="Length target/tol (mm)"
            value={
              info.length.has_target ? `${info.length.target_mm}/${info.length.tolerance_mm}` : ""
            }
            width={90}
            onCommit={(v) => cmd("length_target", v)}
          />
          <label>
            Tuner
            <select
              value={info.length.style}
              aria-label="Tuner style"
              onChange={(e) => cmd("tune_style", e.target.value)}
            >
              <option value="serpentine">Serpentine</option>
              <option value="accordion">Accordion</option>
            </select>
          </label>
          <CommitInput
            label="Amplitude (mm)"
            value={info.length.amplitude_mm}
            width={56}
            onCommit={(v) => cmd("tune_amplitude", v)}
          />
          <CommitInput
            label="Pitch (mm)"
            value={info.length.pitch_mm}
            width={56}
            onCommit={(v) => cmd("tune_pitch", v)}
          />
          {info.length.problem && (
            <span className="warn" role="status">
              {info.length.problem}
            </span>
          )}
        </>
      )}
      {info.route?.pair && (
        <span
          role="status"
          className={info.route.pair.over_skew ? "warn" : "route-status"}
          title="Length of each leg, their difference and the smallest gap, live. Corners make the outer leg longer; length tuning removes the difference (roadmap 24)."
        >
          {info.route.pair.name}: + {info.route.pair.length_p_mm} mm · −{" "}
          {info.route.pair.length_n_mm} mm · Δ {info.route.pair.skew_mm} mm (max{" "}
          {info.route.pair.max_skew_mm}) · gap {info.route.pair.gap_mm || "–"} mm (target{" "}
          {info.route.pair.gap_target_mm})
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
