import { useUi } from "../store";
import { formatLength, gridLabel, unitLabel } from "../units";
import { MODE_TITLES } from "../tools";
import { useActions } from "./hooks";

export function StatusBar() {
  const snap = useUi((s) => s.snapshot);
  const ptr = useUi((s) => s.pointer);
  const hint = useUi((s) => s.hint);
  const actions = useActions();
  if (!snap) return <footer className="statusbar" />;
  const u = snap.units;
  const active = snap.layers.find((l) => l.active);
  const layerText =
    snap.mode === "pcb"
      ? active
        ? active.name
        : "—"
      : snap.mode === "schematic"
        ? "Sheet: Root"
        : "—";
  return (
    <footer className="statusbar" role="status">
      <span className="sb-item strong">
        {snap.project ? `${snap.project.name}${snap.project.dirty ? " •" : ""}` : "No project"}
      </span>
      <span className="sb-item mono" title="Cursor position (snapped)">
        X {ptr ? formatLength(ptr.cursor_x_nm, u) : "—"} Y{" "}
        {ptr ? formatLength(ptr.cursor_y_nm, u) : "—"} {unitLabel(u)}
      </span>
      <span className="sb-item" title="Active snap">
        Snap: {ptr?.snap ?? "—"}
      </span>
      <button
        className="sb-item sb-btn"
        onClick={actions.cycleGrid}
        title="Grid size (click to cycle, Alt+G)"
      >
        Grid {gridLabel(snap.grid.spacing_nm, u)}
      </button>
      <span className="sb-item mono" title="Zoom (100% = real size at 96 dpi)">
        Zoom {`${(ptr?.zoom_percent ?? snap.zoom_percent).toFixed(0)}%`}
      </span>
      <button
        className="sb-item sb-btn"
        onClick={actions.toggleUnits}
        title="Units (click to toggle, Ctrl+Shift+U)"
      >
        Units {unitLabel(u)}
      </button>
      <span className="sb-item" title="Active layer">
        Layer: {layerText}
      </span>
      <span className="sb-item grow muted" title={hint}>
        {ptr?.hover ? `▸ ${ptr.hover}` : hint || MODE_TITLES[snap.mode]}
      </span>
      <span className="sb-item muted" title={snap.gpu_error ?? "GPU adapter"}>
        {snap.gpu_error
          ? "GPU: unavailable"
          : snap.gpu_adapter
            ? `GPU: ${snap.gpu_adapter}`
            : "GPU: idle"}
      </span>
    </footer>
  );
}
