import { useState } from "react";
import { api } from "../api";
import { call } from "../actions";
import type { ViolationInfo } from "../bindings/ViolationInfo";
import { store, useUi } from "../store";

const SEVERITIES = ["error", "warning", "info"] as const;
type Severity = (typeof SEVERITIES)[number];
const LABEL: Record<Severity, string> = { error: "Errors", warning: "Warnings", info: "Info" };

function Row({ v }: { v: ViolationInfo }) {
  return (
    <li className={`erc-row sev-${v.severity}${v.waived ? " waived" : ""}`}>
      <button
        className="erc-main"
        disabled={!v.can_focus}
        title={v.can_focus ? "Select and centre in the schematic" : "Not on the visible sheet"}
        onClick={() => void call(api.ercFocus(v.index))}
      >
        <span className="erc-sev">{v.severity}</span>
        <span className="erc-title">{v.title}</span>
        <span className="erc-msg">{v.message}</span>
        {v.sheet && <span className="erc-sheet muted">{v.sheet}</span>}
      </button>
      {v.waived ? (
        <span className="erc-waiver">
          <span className="muted" title={v.waived}>
            Waived: {v.waived}
          </span>{" "}
          <button className="link" onClick={() => void call(api.ercUnwaive(v.fingerprint))}>
            remove waiver
          </button>
        </span>
      ) : (
        <button
          className="link"
          onClick={() =>
            store.set({
              dialog: { kind: "erc-waive", fingerprint: v.fingerprint, message: v.message },
            })
          }
        >
          Waive…
        </button>
      )}
    </li>
  );
}

/** Electrical Rules Check results. All checking happens in the Rust core. */
export function ErcPanel() {
  const snap = useUi((s) => s.snapshot);
  const [shown, setShown] = useState<Record<Severity, boolean>>({
    error: true,
    warning: true,
    info: false,
  });
  const [showWaived, setShowWaived] = useState(true);
  const erc = snap?.erc;
  if (!snap?.project) return <p className="muted">No project is open.</p>;
  const list = (erc?.violations ?? []).filter(
    (v) => (v.waived ? showWaived : shown[v.severity as Severity]) ?? true,
  );
  return (
    <div className="erc">
      <div className="erc-bar">
        <button className="primary" onClick={() => void call(api.ercRun())}>
          {erc ? "Run again" : "Run ERC"}
        </button>
        {erc && (
          <>
            {SEVERITIES.map((s) => (
              <label key={s} className={`erc-chip sev-${s}`}>
                <input
                  type="checkbox"
                  checked={shown[s]}
                  onChange={(e) => setShown({ ...shown, [s]: e.target.checked })}
                />
                {LABEL[s]} {s === "error" ? erc.errors : s === "warning" ? erc.warnings : erc.infos}
              </label>
            ))}
            <label className="erc-chip">
              <input
                type="checkbox"
                checked={showWaived}
                onChange={(e) => setShowWaived(e.target.checked)}
              />
              Waived {erc.waived}
            </label>
            {erc.stale && (
              <span className="warn" role="status">
                The design changed since this run — run again.
              </span>
            )}
          </>
        )}
      </div>
      {!erc && (
        <p className="muted">
          ERC has not been run. It checks pin compatibility, unconnected and undriven pins,
          references, wires and component data on the electrical connectivity graph.
        </p>
      )}
      {erc && list.length === 0 && (
        <p className="muted">
          {erc.errors + erc.warnings + erc.infos + erc.waived === 0
            ? "No findings."
            : "Nothing to show with the current filters."}
        </p>
      )}
      <ul className="erc-list">
        {list.map((v) => (
          <Row key={`${v.index}`} v={v} />
        ))}
      </ul>
      {erc && erc.truncated > 0 && (
        <p className="muted">…and {erc.truncated} more findings not listed.</p>
      )}
    </div>
  );
}
