import { save } from "@tauri-apps/plugin-dialog";
import { api } from "../api";
import { call } from "../actions";
import { useUi } from "../store";

/**
 * Manufacturing preview and export. DRC, the preflight and every file are produced by the Rust
 * core on the real board; preflight errors block `production.zip`.
 */
export function FabPanel() {
  const snap = useUi((s) => s.snapshot);
  const fab = snap?.fab;
  if (!snap?.project) return <p className="muted">No project is open.</p>;

  const exportZip = async () => {
    const path = await save({
      title: "Save production.zip",
      defaultPath: "production.zip",
      filters: [{ name: "ZIP archive", extensions: ["zip"] }],
    });
    if (path) await call(api.fabExport(path));
  };

  return (
    <div className="erc">
      <div className="erc-bar">
        <button className="primary" onClick={() => void call(api.fabPreview())}>
          {fab ? "Run preflight again" : "Preview manufacturing"}
        </button>
        <button
          disabled={!fab || fab.blocked}
          title={
            !fab
              ? "Run the preview first"
              : fab.blocked
                ? "Fix the preflight errors first"
                : "Write production.zip"
          }
          onClick={() => void exportZip()}
        >
          Export production.zip…
        </button>
        {fab && (
          <>
            <span className="erc-chip sev-error">Errors {fab.errors}</span>
            <span className="erc-chip sev-warning">Warnings {fab.warnings}</span>
            {fab.stale && (
              <span className="warn" role="status">
                The board changed since this preview — run it again.
              </span>
            )}
          </>
        )}
      </div>
      {!fab && (
        <p className="muted">
          The preview runs DRC and the manufacturing preflight, then lists the Gerber, Excellon,
          BOM, pick-and-place and assembly files that would go into production.zip. A successful
          export is not fabrication sign-off.
        </p>
      )}
      <ul className="erc-list">
        {(fab?.issues ?? []).map((i, n) => (
          <li key={n} className={`erc-row sev-${i.severity === "note" ? "info" : i.severity}`}>
            <span className="erc-main">
              <span className="erc-sev">{i.severity}</span>
              <span className="erc-title">{i.code}</span>
              <span className="erc-msg">{i.message}</span>
            </span>
          </li>
        ))}
      </ul>
      {fab && (
        <table className="fab-files">
          <thead>
            <tr>
              <th>File</th>
              <th>Size</th>
              <th>Contents</th>
            </tr>
          </thead>
          <tbody>
            {fab.files.map((f) => (
              <tr key={f.name}>
                <td>{f.name}</td>
                <td>{f.bytes} B</td>
                <td>{f.summary}</td>
              </tr>
            ))}
          </tbody>
        </table>
      )}
    </div>
  );
}
