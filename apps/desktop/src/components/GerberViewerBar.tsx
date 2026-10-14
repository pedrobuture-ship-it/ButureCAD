import { open } from "@tauri-apps/plugin-dialog";
import { api } from "../api";
import { call, pickFolder } from "../actions";
import { useUi } from "../store";

/**
 * Controls of the Gerber viewer. The files are parsed and drawn in Rust; what is shown comes from
 * the Gerber/Excellon bytes, not from the board model.
 */
export function GerberViewerBar() {
  const info = useUi((s) => s.snapshot?.gerber);
  if (!info) return null;
  const cmd = (name: string, arg?: string) => void call(api.gerberCommand(name, arg));
  const openZip = async () => {
    const p = await open({
      multiple: false,
      title: "Open production.zip",
      filters: [{ name: "ZIP archive", extensions: ["zip"] }],
    });
    if (typeof p === "string") cmd("open", p);
  };
  const openFolder = async () => {
    const p = await pickFolder("Open a folder of Gerber and drill files");
    if (p) cmd("open", p);
  };
  const failed = info.checks.filter((c) => !c.ok).length;
  return (
    <div className="gerber-bar" role="toolbar" aria-label="Gerber viewer">
      <div className="symbol-bar">
        <button className="primary" onClick={() => cmd("load_preview")}>
          Load current export
        </button>
        <button onClick={() => void openZip()}>Open zip…</button>
        <button onClick={() => void openFolder()}>Open folder…</button>
        <button disabled={!info.loaded} onClick={() => cmd("fit")}>
          Fit
        </button>
        <button disabled={!info.loaded} onClick={() => cmd("all", "1")}>
          Show all
        </button>
        <button disabled={!info.loaded} onClick={() => cmd("all", "0")}>
          Hide all
        </button>
        <span className="muted">
          {info.loaded
            ? `Source: ${info.source}. Drag: pan · wheel: zoom. This view parses the files, not the board.`
            : "Nothing loaded. “Load current export” generates the manufacturing files and reads them back."}
        </span>
      </div>
      {info.errors.map((e) => (
        <div key={e} className="warn" role="status">
          {e}
        </div>
      ))}
      {info.loaded && (
        <div className="symbol-bar">
          {info.layers.map((l, i) => (
            <label key={l.name} title={l.summary}>
              <input
                type="checkbox"
                checked={l.visible}
                onChange={(e) => cmd("toggle", `${i}:${e.target.checked ? 1 : 0}`)}
              />
              <span style={{ color: l.color }}>■</span>{" "}
              <a
                href="#"
                onClick={(e) => {
                  e.preventDefault();
                  cmd("select", String(i));
                }}
                style={{ fontWeight: info.selected === i ? 700 : 400 }}
              >
                {l.name}
              </a>
            </label>
          ))}
        </div>
      )}
      {info.loaded && info.selected !== null && (
        <div className="symbol-bar">
          <span className="muted">{info.layers[info.selected]?.summary}</span>
          {info.layers[info.selected]?.warnings.map((w) => (
            <span key={w} className="warn">
              {w}
            </span>
          ))}
          {info.apertures.length > 0 && (
            <span>
              Apertures: {info.apertures.map((a) => `${a.code} ${a.shape} ×${a.uses}`).join(" · ")}
            </span>
          )}
          {info.tools.length > 0 && (
            <span>
              Tools:{" "}
              {info.tools.map((t) => `${t.tool} Ø${t.diameter_mm} mm ×${t.holes}`).join(" · ")}
            </span>
          )}
        </div>
      )}
      {info.checks.length > 0 && (
        <details open={failed > 0}>
          <summary className={failed > 0 ? "warn" : "muted"}>
            Verification against the project: {info.checks.length - failed}/{info.checks.length}{" "}
            checks pass
          </summary>
          <ul>
            {info.checks.map((c) => (
              <li key={c.text} className={c.ok ? "muted" : "warn"}>
                {c.ok ? "✓" : "✗"} {c.text}
              </li>
            ))}
          </ul>
        </details>
      )}
    </div>
  );
}
