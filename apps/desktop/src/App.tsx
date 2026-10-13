import { useEffect, useState } from "react";
import { coreInfo, type CoreInfo } from "./api";

const MODULES = ["Schematic", "PCB", "Library", "3D / MCAD", "Simulation", "DRC", "Manufacturing"];

/** UI shell only. No EDA functionality exists yet; see docs/STATUS.md. */
export function App() {
  const [info, setInfo] = useState<CoreInfo | null>(null);
  const [error, setError] = useState<string | null>(null);

  useEffect(() => {
    coreInfo()
      .then(setInfo)
      .catch((e: unknown) => setError(String(e)));
  }, []);

  return (
    <div className="shell">
      <aside className="sidebar">
        <h1>ButureCAD</h1>
        <nav>
          {MODULES.map((m) => (
            <div key={m} className="nav-item disabled" title="Not implemented yet">
              {m}
            </div>
          ))}
        </nav>
      </aside>
      <main className="main">
        <p>Foundation stage. No editor is implemented yet.</p>
      </main>
      <footer className="status">
        {info ? `${info.name} v${info.version} (core)` : (error ?? "connecting to core…")}
      </footer>
    </div>
  );
}
