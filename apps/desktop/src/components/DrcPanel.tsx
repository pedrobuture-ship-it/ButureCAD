import { api } from "../api";
import { call } from "../actions";
import { useUi } from "../store";

/** Design Rules Check results. All checking happens in the Rust core on the real board geometry. */
export function DrcPanel() {
  const snap = useUi((s) => s.snapshot);
  const drc = snap?.drc;
  if (!snap?.project) return <p className="muted">No project is open.</p>;
  return (
    <div className="erc">
      <div className="erc-bar">
        <button className="primary" onClick={() => void call(api.drcRun())}>
          {drc ? "Run again" : "Run DRC"}
        </button>
        {drc && (
          <>
            <span className="erc-chip sev-error">Errors {drc.errors}</span>
            <span className="erc-chip sev-warning">Warnings {drc.warnings}</span>
            {drc.stale && (
              <span className="warn" role="status">
                The board changed since this run — run again.
              </span>
            )}
          </>
        )}
      </div>
      {!drc && (
        <p className="muted">
          DRC has not been run. It checks clearance, shorts, track width, vias, copper-to-edge,
          unrouted connections and courtyards on the board geometry. It does not check zones (not
          implemented yet) and is not a fabrication sign-off.
        </p>
      )}
      {drc && drc.violations.length === 0 && <p className="muted">No violations.</p>}
      <ul className="erc-list">
        {(drc?.violations ?? []).map((v) => (
          <li key={v.index} className={`erc-row sev-${v.severity}`}>
            <button
              className="erc-main"
              title="Select the objects and zoom to the problem"
              onClick={() => void call(api.drcFocus(v.index))}
            >
              <span className="erc-sev">{v.severity}</span>
              <span className="erc-title">{v.title}</span>
              <span className="erc-msg">{v.message}</span>
            </button>
          </li>
        ))}
      </ul>
      {drc && drc.truncated > 0 && (
        <p className="muted">…and {drc.truncated} more violations not listed.</p>
      )}
    </div>
  );
}
