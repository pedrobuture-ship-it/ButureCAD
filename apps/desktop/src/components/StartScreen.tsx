import { openProjectAt } from "../actions";
import { useUi } from "../store";
import { useActions } from "./hooks";

export function StartScreen() {
  const snap = useUi((s) => s.snapshot);
  const actions = useActions();
  return (
    <div className="start">
      <h1>ButureCAD</h1>
      <p className="muted">Design visually. Design with code. Everything stays on this machine.</p>
      <div className="start-actions">
        <button className="primary" onClick={actions.newProject}>
          New project…
        </button>
        <button onClick={actions.openProject}>Open project…</button>
      </div>
      <h3>Recent projects</h3>
      {(snap?.recent.length ?? 0) === 0 ? (
        <p className="muted">No recent projects.</p>
      ) : (
        <ul className="recent">
          {snap?.recent.map((r) => (
            <li key={r.path}>
              <button className="link" onClick={() => void openProjectAt(r.path)} title={r.path}>
                {r.name}
              </button>
              <span className="muted path">{r.path}</span>
            </li>
          ))}
        </ul>
      )}
    </div>
  );
}
