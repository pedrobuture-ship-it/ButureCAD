import { useCallback, useEffect, useRef, useState } from "react";
import { api } from "../api";
import { call, pickFolder, toast } from "../actions";
import { useUi } from "../store";
import { CanvasView } from "./CanvasView";

/**
 * ButureScript editor: CODE, SPLIT and VISUAL views. The source is edited here; parsing,
 * validation, the EDA IR and the preview all happen in the Rust core on every (debounced) change.
 * The preview is read-only and shows what the core builds from the script.
 */
export function CodePanel() {
  const info = useUi((s) => s.snapshot?.code);
  const [text, setText] = useState("");
  const [loaded, setLoaded] = useState(false);
  const [folder, setFolder] = useState("generated");
  const area = useRef<HTMLTextAreaElement>(null);
  const gutter = useRef<HTMLPreElement>(null);
  const timer = useRef<ReturnType<typeof setTimeout> | undefined>(undefined);

  const hasProject = info?.has_project ?? false;
  useEffect(() => {
    if (!hasProject || loaded) return;
    api
      .scriptSource()
      .then((t) => {
        setText(t);
        setLoaded(true);
        return call(api.scriptCommand("update", t));
      })
      .catch((e) => toast(String(e)));
  }, [hasProject, loaded]);

  const change = useCallback((t: string) => {
    setText(t);
    clearTimeout(timer.current);
    timer.current = setTimeout(() => void call(api.scriptCommand("update", t)), 350);
  }, []);

  if (!info) return null;
  const jump = (line: number, col: number) => {
    const el = area.current;
    if (!el) return;
    const lines = text.split("\n");
    let off = 0;
    for (let i = 0; i < line - 1 && i < lines.length; i++) off += lines[i].length + 1;
    off += Math.max(0, col - 1);
    el.focus();
    el.setSelectionRange(off, off);
  };
  const errors = info.diagnostics.filter((d) => d.severity === "error");
  const lineCount = Math.max(1, text.split("\n").length);
  const bad = new Set(errors.map((d) => d.line));
  const showCode = info.view !== "visual";
  const showVisual = info.view !== "code";
  const save = () => void call(api.scriptCommand("save", text));
  const generate = async () => {
    const parent = await pickFolder("Choose where to create the project folder");
    if (parent) await call(api.scriptCommand("generate", `${parent}/${folder}|${text}`));
  };
  return (
    <div className="libmgr" style={{ height: "100%" }}>
      <div className="symbol-bar">
        {(["code", "split", "visual"] as const).map((v) => (
          <button
            key={v}
            className={info.view === v ? "primary" : ""}
            onClick={() => void call(api.scriptCommand("view", v))}
          >
            {v.toUpperCase()}
          </button>
        ))}
        {showVisual && (
          <>
            {(["schematic", "pcb", "3d"] as const).map((p) => (
              <button
                key={p}
                className={info.preview === p ? "primary" : ""}
                onClick={() => void call(api.scriptCommand("preview", p))}
              >
                {p === "3d" ? "3D" : p === "pcb" ? "PCB" : "Schematic"}
              </button>
            ))}
            <button onClick={() => void call(api.scriptCommand("fit"))}>Fit</button>
          </>
        )}
        <button
          disabled={!hasProject}
          onClick={save}
          title="Write design.beda in the project folder"
        >
          Save{info.dirty ? " •" : ""}
        </button>
        <label>
          Folder
          <input
            value={folder}
            onChange={(e) => setFolder(e.target.value)}
            style={{ width: 110 }}
          />
        </label>
        <button
          disabled={errors.length > 0}
          onClick={() => void generate()}
          title="Create a real project from the script and open it in the visual editors"
        >
          Generate project…
        </button>
        <span className="muted">
          {errors.length > 0
            ? `${errors.length} error(s) — the preview shows the last good version`
            : info.summary || "…"}
        </span>
      </div>
      {!hasProject ? (
        <p className="muted" style={{ padding: 12 }}>
          Open a project first: the script is saved as design.beda next to it.
        </p>
      ) : (
        <div className={`code-body view-${info.view}`}>
          {showCode && (
            <div className="code-left">
              <div className="code-editor">
                <pre className="code-gutter" ref={gutter} aria-hidden="true">
                  {Array.from({ length: lineCount }, (_, i) => (
                    <span key={i} className={bad.has(i + 1) ? "bad" : ""}>
                      {i + 1}
                      {"\n"}
                    </span>
                  ))}
                </pre>
                <textarea
                  ref={area}
                  aria-label="ButureScript source"
                  className="code-text"
                  spellCheck={false}
                  wrap="off"
                  value={text}
                  onChange={(e) => change(e.target.value)}
                  onScroll={(e) => {
                    if (gutter.current) gutter.current.scrollTop = e.currentTarget.scrollTop;
                  }}
                  onKeyDown={(e) => {
                    if ((e.ctrlKey || e.metaKey) && e.key === "s") {
                      e.preventDefault();
                      e.stopPropagation();
                      save();
                    }
                  }}
                />
              </div>
              <ul className="erc-list code-diags">
                {info.diagnostics.length === 0 && <li className="muted">No problems.</li>}
                {info.diagnostics.map((d, i) => (
                  <li key={i} className={`erc-row sev-${d.severity}`}>
                    <button className="erc-main" onClick={() => jump(d.line, d.col)}>
                      <span className="erc-sev">
                        {d.line}:{d.col}
                      </span>
                      <span className="erc-title">{d.code}</span>
                      <span className="erc-msg">{d.message}</span>
                    </button>
                  </li>
                ))}
              </ul>
            </div>
          )}
          {showVisual && (
            <div className="code-right">
              {info.has_preview ? (
                <CanvasView />
              ) : (
                <p className="muted" style={{ padding: 12 }}>
                  No valid preview yet: fix the errors on the left.
                </p>
              )}
            </div>
          )}
        </div>
      )}
    </div>
  );
}
