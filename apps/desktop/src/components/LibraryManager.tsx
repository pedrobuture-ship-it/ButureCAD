import { useState } from "react";
import { api } from "../api";
import { call, toast } from "../actions";
import { useUi } from "../store";

const KINDS = ["symbol", "footprint", "model", "component"] as const;

/**
 * Library Manager. Searching, filtering, favorites and the libraries themselves live in the Rust
 * core (a local index over the library folders); this panel only sends actions.
 */
export function LibraryManager() {
  const info = useUi((s) => s.snapshot?.library_manager);
  const [form, setForm] = useState<null | "component" | "model">(null);
  if (!info) return null;
  const cmd = (name: string, arg?: string) => void call(api.libraryCommand(name, arg));
  const d = info.detail;
  const place = async (key: string) => {
    try {
      const id = await api.libraryPlaceSymbol(key);
      await call(api.setMode("schematic"));
      await call(api.editorCommand(`tool:place_component:${id}`));
    } catch (e) {
      toast(String(e));
    }
  };
  return (
    <div className="libmgr">
      <div className="symbol-bar">
        <input
          aria-label="Search libraries"
          placeholder="Search symbols, footprints, models and components…"
          defaultValue={info.text}
          onChange={(e) => cmd("query", e.target.value)}
          style={{ minWidth: 280 }}
        />
        {KINDS.map((k) => (
          <label key={k}>
            <input
              type="checkbox"
              checked={info.kinds.includes(k)}
              onChange={(e) => cmd("kind", `${k}:${e.target.checked ? 1 : 0}`)}
            />
            {k}
          </label>
        ))}
        <select
          aria-label="Library"
          value={info.library}
          onChange={(e) => cmd("library", e.target.value)}
        >
          <option value="">All libraries</option>
          {info.libraries.map((l) => (
            <option key={l}>{l}</option>
          ))}
        </select>
        <select
          aria-label="Manufacturer"
          value={info.manufacturer}
          onChange={(e) => cmd("manufacturer", e.target.value)}
        >
          <option value="">Any manufacturer</option>
          {info.manufacturers.map((m) => (
            <option key={m}>{m}</option>
          ))}
        </select>
        {(["all", "favorites", "recents"] as const).map((s) => (
          <button
            key={s}
            className={info.scope === s ? "primary" : ""}
            onClick={() => cmd("scope", s)}
          >
            {s}
          </button>
        ))}
        <button onClick={() => cmd("refresh")} title="Re-read the library folders">
          Refresh
        </button>
        <button onClick={() => setForm(form === "component" ? null : "component")}>
          New component…
        </button>
        <button onClick={() => setForm(form === "model" ? null : "model")}>Add 3D model…</button>
        <span className="muted">
          {info.results.length}
          {info.truncated ? "+" : ""} of {info.total} items
          {info.prefs_persisted ? "" : " · favorites are not saved (no user library folder)"}
        </span>
      </div>
      {info.problems.map((p) => (
        <div key={p} className="warn" role="status">
          {p}
        </div>
      ))}
      {form && <NewItemForm kind={form} scopes={info.writable_scopes} done={() => setForm(null)} />}
      <div className="libmgr-body">
        <ul className="symbol-list" role="listbox" aria-label="Results">
          {info.results.length === 0 && <li className="muted">Nothing matches.</li>}
          {info.results.map((r) => (
            <li
              key={r.key}
              role="option"
              aria-selected={d?.row.key === r.key}
              className={d?.row.key === r.key ? "sel" : ""}
              onClick={() => cmd("select", r.key)}
            >
              <button
                className="icon"
                aria-label={r.favorite ? "Remove favorite" : "Add favorite"}
                onClick={(e) => {
                  e.stopPropagation();
                  cmd("favorite", r.key);
                }}
              >
                {r.favorite ? "★" : "☆"}
              </button>
              <strong>{r.name}</strong>
              <span className="muted">
                {r.kind} · {r.library} ({r.scope}){r.manufacturer ? ` · ${r.manufacturer}` : ""}
                {r.description ? ` · ${r.description}` : ""}
              </span>
            </li>
          ))}
        </ul>
        <div className="libmgr-detail">
          {!d && <p className="muted">Select an item to see its details.</p>}
          {d && (
            <>
              <h3>
                {d.row.name} <span className="muted">{d.row.kind}</span>
              </h3>
              <dl>
                <dt>Id</dt>
                <dd>{d.row.id}</dd>
                {d.row.description && (
                  <>
                    <dt>Description</dt>
                    <dd>{d.row.description}</dd>
                  </>
                )}
                {d.symbol && (
                  <>
                    <dt>Symbol</dt>
                    <dd>{d.symbol}</dd>
                  </>
                )}
                {(d.footprint || d.default_footprint) && (
                  <>
                    <dt>Footprint</dt>
                    <dd>{d.footprint || `${d.default_footprint} (default)`}</dd>
                  </>
                )}
                {d.model3d && (
                  <>
                    <dt>3D model</dt>
                    <dd>{d.model3d}</dd>
                  </>
                )}
                {d.row.manufacturer && (
                  <>
                    <dt>Manufacturer</dt>
                    <dd>{d.row.manufacturer}</dd>
                  </>
                )}
                {d.row.mpn && (
                  <>
                    <dt>MPN</dt>
                    <dd>{d.row.mpn}</dd>
                  </>
                )}
                {d.datasheet && (
                  <>
                    <dt>Datasheet</dt>
                    <dd>{d.datasheet} (stored reference; not opened or fetched)</dd>
                  </>
                )}
                {d.parameters.map((p) => (
                  <div key={p.key}>
                    <dt>{p.key}</dt>
                    <dd>{p.value}</dd>
                  </div>
                ))}
              </dl>
              {d.unresolved.map((u) => (
                <p key={u} className="warn">
                  {u}
                </p>
              ))}
              <div className="symbol-bar">
                <button disabled={!d.placeable} onClick={() => void place(d.row.key)}>
                  Place symbol
                </button>
                {d.row.kind === "component" && d.row.scope !== "builtin" && (
                  <button
                    onClick={() =>
                      cmd("component_delete", `${d.row.scope}|${d.row.library}|${d.row.name}`)
                    }
                  >
                    Delete component
                  </button>
                )}
              </div>
              {d.row.kind === "component" && (
                <p className="muted">
                  Placing uses the component&apos;s symbol; its footprint, value and manufacturer
                  data are not copied to the schematic yet.
                </p>
              )}
            </>
          )}
        </div>
      </div>
    </div>
  );
}

function NewItemForm({
  kind,
  scopes,
  done,
}: {
  kind: "component" | "model";
  scopes: string[];
  done: () => void;
}) {
  const [v, setV] = useState({
    scope: scopes[scopes.length - 1] ?? "user",
    library: "",
    name: "",
    description: "",
    symbol: "",
    footprint: "",
    model3d: "",
    manufacturer: "",
    mpn: "",
    datasheet: "",
    path: "",
    params: "",
    tags: "",
  });
  const set = (k: keyof typeof v) => (e: { target: { value: string } }) =>
    setV({ ...v, [k]: e.target.value });
  const save = async () => {
    const parameters: Record<string, string> = {};
    for (const line of v.params.split("\n")) {
      const i = line.indexOf("=");
      if (i > 0) parameters[line.slice(0, i).trim()] = line.slice(i + 1).trim();
    }
    const body =
      kind === "component"
        ? {
            scope: v.scope,
            library: v.library,
            name: v.name,
            description: v.description,
            symbol: v.symbol,
            footprint: v.footprint,
            model3d: v.model3d,
            manufacturer: v.manufacturer,
            mpn: v.mpn,
            datasheet: v.datasheet,
            parameters,
            tags: v.tags
              .split(",")
              .map((t) => t.trim())
              .filter(Boolean),
          }
        : {
            scope: v.scope,
            library: v.library,
            name: v.name,
            path: v.path,
            description: v.description,
          };
    await call(
      api.libraryCommand(
        kind === "component" ? "component_save" : "model_save",
        JSON.stringify(body),
      ),
    );
    done();
  };
  const field = (label: string, k: keyof typeof v, placeholder = "") => (
    <label>
      {label}
      <input value={v[k]} placeholder={placeholder} onChange={set(k)} />
    </label>
  );
  return (
    <div className="symbol-bar libmgr-form">
      <label>
        Folder
        <select value={v.scope} onChange={set("scope")}>
          {scopes.map((s) => (
            <option key={s}>{s}</option>
          ))}
        </select>
      </label>
      {field("Library", "library", "Parts")}
      {field("Name", "name")}
      {field("Description", "description")}
      {kind === "component" ? (
        <>
          {field("Symbol", "symbol", "Library:Name")}
          {field("Footprint", "footprint", "Library:Name")}
          {field("3D model", "model3d", "path or Library:Name")}
          {field("Manufacturer", "manufacturer")}
          {field("MPN", "mpn")}
          {field("Datasheet", "datasheet", "path or URL (not fetched)")}
          {field("Tags", "tags", "comma separated")}
          <label>
            Parameters (key=value per line)
            <textarea value={v.params} onChange={set("params")} rows={2} />
          </label>
        </>
      ) : (
        field("File path", "path", "models/part.step")
      )}
      <button className="primary" onClick={() => void save()}>
        Save
      </button>
      <button onClick={done}>Cancel</button>
    </div>
  );
}
