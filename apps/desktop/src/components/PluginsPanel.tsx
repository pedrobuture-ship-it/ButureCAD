import { open } from "@tauri-apps/plugin-dialog";
import { useState } from "react";
import { api } from "../api";
import { call } from "../actions";
import { useUi } from "../store";

/**
 * Plugin Manager and component-provider browser. Plugins run in a WebAssembly sandbox in the Rust
 * core with only the permissions granted here; this panel only sends actions.
 */
export function PluginsPanel() {
  const info = useUi((s) => s.snapshot?.plugins);
  const [query, setQuery] = useState("");
  const [library, setLibrary] = useState("Imported");
  if (!info) return null;
  const cmd = (name: string, arg?: string) => void call(api.pluginCommand(name, arg));
  const install = async () => {
    const p = await open({
      multiple: false,
      title: "Install a plugin (choose its plugin.json)",
      filters: [{ name: "Plugin manifest", extensions: ["json"] }],
    });
    if (typeof p === "string") cmd("install", p);
  };
  return (
    <div className="libmgr">
      <div className="symbol-bar">
        <button className="primary" disabled={!info.available} onClick={() => void install()}>
          Install from file…
        </button>
        <span className="muted">
          {info.available
            ? "Plugins are WebAssembly modules without file, network or project access unless you grant it below. A newly installed plugin is disabled and has no permission."
            : "No plugin folder is available in this session."}
        </span>
      </div>
      <div style={{ padding: 8 }}>
        {info.plugins.length === 0 && <p className="muted">No plugins installed.</p>}
        {info.plugins.map((p) => (
          <div key={p.id} className="plugin-card">
            <h3>
              {p.name}{" "}
              <span className="muted">
                {p.id} · v{p.version}
              </span>
            </h3>
            {p.description && <p>{p.description}</p>}
            <p className="muted">Provides: {p.categories.join(", ") || "—"}</p>
            {p.error && (
              <p className="warn" role="status">
                {p.error}
              </p>
            )}
            <div className="symbol-bar">
              <label>
                <input
                  type="checkbox"
                  checked={p.enabled}
                  onChange={(e) => cmd(e.target.checked ? "enable" : "disable", p.id)}
                />
                Enabled {p.enabled && !p.loaded ? "(not loaded)" : ""}
              </label>
              <button disabled={!p.enabled} onClick={() => cmd("reload", p.id)}>
                Reload
              </button>
              <button onClick={() => cmd("remove", p.id)}>Remove</button>
            </div>
            <div className="symbol-bar">
              <span>Permissions:</span>
              {p.requested.length === 0 && <span className="muted">none requested</span>}
              {p.requested.map((perm) => (
                <label key={perm} title="Takes effect on the plugin's next call">
                  <input
                    type="checkbox"
                    checked={p.granted.includes(perm)}
                    onChange={(e) => cmd(e.target.checked ? "grant" : "revoke", `${p.id}|${perm}`)}
                  />
                  {perm}
                </label>
              ))}
            </div>
            {p.requested.includes("network") && (
              <p className="muted">
                The network permission can be granted and revoked, but this build has no network
                client behind it yet: such requests fail with an explicit error.
              </p>
            )}
            {p.requested.includes("project-write") && (
              <p className="muted">
                project-write does not let a plugin edit the project directly; the proposal flow is
                not implemented yet.
              </p>
            )}
          </div>
        ))}
      </div>
      {info.providers.length > 0 && (
        <div style={{ padding: 8 }}>
          <h3>Component providers</h3>
          <div className="symbol-bar">
            <select
              aria-label="Provider"
              value={info.provider}
              onChange={(e) => cmd("provider", e.target.value)}
            >
              <option value="">Choose a provider…</option>
              {info.providers.map((p) => (
                <option key={p}>{p}</option>
              ))}
            </select>
            <input
              aria-label="Search the provider"
              placeholder="Search the provider…"
              value={query}
              onChange={(e) => setQuery(e.target.value)}
              onKeyDown={(e) => e.key === "Enter" && cmd("search", query)}
            />
            <button disabled={!info.provider} onClick={() => cmd("search", query)}>
              Search
            </button>
            <label>
              Import into library
              <input value={library} onChange={(e) => setLibrary(e.target.value)} />
            </label>
          </div>
          {info.error && (
            <p className="warn" role="status">
              {info.error}
            </p>
          )}
          {info.audit.map((a) => (
            <p key={a} className="warn">
              Refused: {a}
            </p>
          ))}
          {info.last_import && <p className="muted">{info.last_import}</p>}
          <ul className="symbol-list">
            {info.hits.map((h) => (
              <li key={h.id}>
                <strong>{h.name}</strong>
                <span className="muted">
                  {h.manufacturer} {h.mpn} {h.description}
                </span>
                <button onClick={() => cmd("import", `${h.id}|${library}`)}>Import</button>
              </li>
            ))}
          </ul>
        </div>
      )}
    </div>
  );
}
