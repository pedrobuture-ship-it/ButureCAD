import { useState } from "react";
import { api } from "../api";
import { call } from "../actions";
import type { SimulationResultInfo } from "../bindings/SimulationResultInfo";
import { useUi } from "../store";

const PALETTE = [
  "#4c8bf5",
  "#e8743b",
  "#2fa77a",
  "#c05cc9",
  "#d1a21d",
  "#d9534f",
  "#2aa5b8",
  "#8a8f98",
];

/** Engineering notation for axis labels. */
function eng(v: number): string {
  if (v === 0) return "0";
  const a = Math.abs(v);
  const units: [number, string][] = [
    [1e9, "G"],
    [1e6, "M"],
    [1e3, "k"],
    [1, ""],
    [1e-3, "m"],
    [1e-6, "µ"],
    [1e-9, "n"],
    [1e-12, "p"],
  ];
  const [scale, suffix] = units.find(([s]) => a >= s) ?? units[units.length - 1];
  const x = v / scale;
  return `${Math.abs(x) >= 100 ? x.toFixed(0) : Math.abs(x) >= 10 ? x.toFixed(1) : x.toFixed(2)}${suffix}`;
}

function niceTicks(lo: number, hi: number, n = 5): number[] {
  if (!(hi > lo)) return [lo];
  const raw = (hi - lo) / n;
  const mag = 10 ** Math.floor(Math.log10(raw));
  const step = [1, 2, 5, 10].map((m) => m * mag).find((s) => s >= raw) ?? raw;
  const out: number[] = [];
  for (let t = Math.ceil(lo / step) * step; t <= hi + step * 1e-9; t += step) out.push(t);
  return out;
}

/** One plot: all visible series that share a unit, drawn as SVG polylines (<= 1500 points each). */
function Plot({
  r,
  unit,
  names,
  colors,
}: {
  r: SimulationResultInfo;
  unit: string;
  names: string[];
  colors: Record<string, string>;
}) {
  const W = 760;
  const H = 240;
  const m = { l: 62, r: 12, t: 10, b: 30 };
  const series = r.series.filter((s) => s.unit === unit && names.includes(s.name));
  if (series.length === 0 || r.x.length < 2) return null;
  const xs = r.x_log ? r.x.map((v) => Math.log10(Math.max(v, 1e-300))) : r.x;
  const [x0, x1] = [Math.min(...xs), Math.max(...xs)];
  let [y0, y1] = [Infinity, -Infinity];
  for (const s of series)
    for (const v of s.values) {
      if (Number.isFinite(v)) {
        y0 = Math.min(y0, v);
        y1 = Math.max(y1, v);
      }
    }
  if (!(y1 > y0)) {
    y0 -= 1;
    y1 += 1;
  }
  const pad = (y1 - y0) * 0.05;
  y0 -= pad;
  y1 += pad;
  const px = (v: number) => m.l + ((v - x0) / (x1 - x0 || 1)) * (W - m.l - m.r);
  const py = (v: number) => H - m.b - ((v - y0) / (y1 - y0)) * (H - m.t - m.b);
  const yt = niceTicks(y0, y1);
  const xt = r.x_log
    ? Array.from({ length: Math.floor(x1) - Math.ceil(x0) + 1 }, (_, i) => Math.ceil(x0) + i)
    : niceTicks(x0, x1, 6);
  return (
    <svg viewBox={`0 0 ${W} ${H}`} className="sim-plot" role="img" aria-label={`${unit} plot`}>
      {yt.map((t) => (
        <g key={`y${t}`}>
          <line x1={m.l} x2={W - m.r} y1={py(t)} y2={py(t)} className="sim-grid" />
          <text x={m.l - 6} y={py(t) + 4} textAnchor="end" className="sim-axis">
            {eng(t)}
          </text>
        </g>
      ))}
      {xt.map((t) => (
        <g key={`x${t}`}>
          <line x1={px(t)} x2={px(t)} y1={m.t} y2={H - m.b} className="sim-grid" />
          <text x={px(t)} y={H - m.b + 16} textAnchor="middle" className="sim-axis">
            {eng(r.x_log ? 10 ** t : t)}
          </text>
        </g>
      ))}
      <text x={4} y={14} className="sim-axis">
        {unit}
      </text>
      <text x={W - m.r} y={H - 4} textAnchor="end" className="sim-axis">
        {r.x_label} [{r.x_unit}]
      </text>
      {series.map((s) => (
        <polyline
          key={s.name}
          fill="none"
          stroke={colors[s.name]}
          strokeWidth={1.6}
          points={s.values.map((v, i) => `${px(xs[i]).toFixed(1)},${py(v).toFixed(1)}`).join(" ")}
        />
      ))}
    </svg>
  );
}

/** Simulation: set up an analysis, run it in the Rust core, plot the result. */
export function SimulationPanel() {
  const info = useUi((s) => s.snapshot?.simulation);
  const [hidden, setHidden] = useState<string[]>([]);
  if (!info) return null;
  const set = (k: string, v: string) => void call(api.simulationCommand("set", `${k}=${v}`));
  const field = (label: string, key: keyof typeof info & string, hint = "") => (
    <label title={hint}>
      {label}
      <input
        defaultValue={String(info[key] ?? "")}
        key={`${key}-${String(info[key])}`}
        onBlur={(e) => set(key, e.target.value)}
        style={{ width: 80 }}
      />
    </label>
  );
  const r = info.result;
  const colors: Record<string, string> = {};
  r?.series.forEach((s, i) => (colors[s.name] = PALETTE[i % PALETTE.length]));
  const visible = (r?.series ?? []).map((s) => s.name).filter((n) => !hidden.includes(n));
  const units = [...new Set((r?.series ?? []).map((s) => s.unit))];
  return (
    <div className="libmgr">
      <div className="symbol-bar">
        <label>
          Analysis
          <select value={info.analysis} onChange={(e) => set("analysis", e.target.value)}>
            <option value="op">Operating point</option>
            <option value="dc">DC sweep</option>
            <option value="ac">AC</option>
            <option value="tran">Transient</option>
          </select>
        </label>
        {info.analysis === "dc" && (
          <>
            <label>
              Source
              <select value={info.source} onChange={(e) => set("source", e.target.value)}>
                {info.sources.map((s) => (
                  <option key={s}>{s}</option>
                ))}
              </select>
            </label>
            {field("Start", "start")}
            {field("Stop", "stop")}
            {field("Step", "step")}
          </>
        )}
        {info.analysis === "ac" && (
          <>
            {field("Points/decade", "ppd")}
            {field("Start Hz", "fstart")}
            {field("Stop Hz", "fstop")}
          </>
        )}
        {info.analysis === "tran" && (
          <>
            {field("Step", "tstep", "seconds, SPICE suffixes (u, m, n)")}
            {field("Stop", "tstop")}
          </>
        )}
        <label>
          Engine
          <select value={info.backend} onChange={(e) => set("backend", e.target.value)}>
            {info.backends.map((b) => (
              <option key={b.id} value={b.id}>
                {b.name}
                {b.available ? "" : " (not installed)"}
              </option>
            ))}
          </select>
        </label>
        <button
          className="primary"
          disabled={info.problems.length > 0}
          onClick={() => void call(api.simulationCommand("run"))}
        >
          Run
        </button>
        <span className="muted">
          SPICE suffixes: k, meg, m (milli), u, n, p. The built-in engine is linear only; diodes,
          transistors and subcircuits need ngspice installed.
        </span>
      </div>
      {info.problems.map((p) => (
        <div key={p} className="warn" role="status">
          {p}
        </div>
      ))}
      {info.error && (
        <div className="warn" role="alert">
          {info.error}
        </div>
      )}
      <div style={{ padding: 8 }}>
        {r && (
          <>
            <p className="muted">
              {r.analysis} · {r.backend} · {r.points} point{r.points === 1 ? "" : "s"}
              {r.decimated ? " (plot decimated)" : ""}
              {r.stale ? " · the schematic changed since this run — run again" : ""}
            </p>
            {r.x.length === 0 ? (
              <table className="fab-files">
                <thead>
                  <tr>
                    <th>Quantity</th>
                    <th>Value</th>
                  </tr>
                </thead>
                <tbody>
                  {r.series.map((s) => (
                    <tr key={s.name}>
                      <td>{s.name}</td>
                      <td>
                        {eng(s.values[0])}
                        {s.unit}
                      </td>
                    </tr>
                  ))}
                </tbody>
              </table>
            ) : (
              <>
                <div className="symbol-bar">
                  {r.series.map((s) => (
                    <label key={s.name}>
                      <input
                        type="checkbox"
                        checked={!hidden.includes(s.name)}
                        onChange={(e) =>
                          setHidden(
                            e.target.checked
                              ? hidden.filter((h) => h !== s.name)
                              : [...hidden, s.name],
                          )
                        }
                      />
                      <span style={{ color: colors[s.name] }}>■</span> {s.name}
                    </label>
                  ))}
                </div>
                {units.map((u) => (
                  <Plot key={u} r={r} unit={u} names={visible} colors={colors} />
                ))}
              </>
            )}
          </>
        )}
        {!r && !info.error && <p className="muted">No result yet. Press Run.</p>}
        {info.notes.length > 0 && (
          <details>
            <summary className="muted">{info.notes.length} note(s) about the circuit</summary>
            <ul>
              {info.notes.map((n) => (
                <li key={n} className="muted">
                  {n}
                </li>
              ))}
            </ul>
          </details>
        )}
        <details>
          <summary className="muted">Generated SPICE netlist</summary>
          <pre>{info.netlist}</pre>
        </details>
      </div>
    </div>
  );
}
