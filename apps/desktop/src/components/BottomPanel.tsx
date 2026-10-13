import { useEffect, useRef } from "react";
import { store, useUi, type BottomTab } from "../store";
import { ErcPanel } from "./ErcPanel";

const TABS: { id: BottomTab; label: string }[] = [
  { id: "erc", label: "ERC" },
  { id: "drc", label: "DRC" },
  { id: "messages", label: "Messages" },
  { id: "console", label: "Console" },
];

function NotImplemented({ what, stage }: { what: string; stage: string }) {
  return (
    <div className="not-impl">
      <strong>{what} is not implemented yet.</strong>
      <span>Planned in {stage}. No results are shown because nothing has been checked.</span>
    </div>
  );
}

export function BottomPanel() {
  const snap = useUi((s) => s.snapshot);
  const tab = useUi((s) => s.bottomTab);
  const open = useUi((s) => s.bottomOpen);
  const bodyRef = useRef<HTMLDivElement>(null);
  const messages = snap?.messages ?? [];

  useEffect(() => {
    const el = bodyRef.current;
    if (el) el.scrollTop = el.scrollHeight;
  }, [messages.length, tab, open, snap?.command_log.length]);

  return (
    <div className={open ? "bottom open" : "bottom"}>
      <div className="bottom-tabs" role="tablist">
        {TABS.map((t) => (
          <button
            key={t.id}
            role="tab"
            aria-selected={tab === t.id}
            className={tab === t.id ? "btab active" : "btab"}
            onClick={() => store.set({ bottomTab: t.id, bottomOpen: true })}
          >
            {t.label}
            {t.id === "erc" && snap?.erc && snap.erc.errors + snap.erc.warnings > 0 && (
              <span className={snap.erc.errors > 0 ? "count err" : "count"}>
                {snap.erc.errors + snap.erc.warnings}
              </span>
            )}
            {t.id === "messages" && messages.length > 0 && (
              <span className="count">{messages.length}</span>
            )}
          </button>
        ))}
        <span className="tb-spacer" />
        <button
          className="btab"
          onClick={() => store.set({ bottomOpen: !open })}
          title="Toggle panel (Ctrl+J)"
        >
          {open ? "Hide" : "Show"}
        </button>
      </div>
      {open && (
        <div className="bottom-body" ref={bodyRef}>
          {tab === "erc" && <ErcPanel />}
          {tab === "drc" && <NotImplemented what="Design Rules Check" stage="roadmap stage 19" />}
          {tab === "messages" && (
            <ul className="log">
              {messages.length === 0 && <li className="muted">No messages.</li>}
              {messages.map((m) => (
                <li key={m.seq} className={`lvl-${m.level}`}>
                  <span className="lvl">{m.level}</span> {m.text}
                </li>
              ))}
            </ul>
          )}
          {tab === "console" && (
            <ul className="log">
              {(snap?.command_log.length ?? 0) === 0 && (
                <li className="muted">
                  No commands executed in this session yet. Executed commands (including undo/redo)
                  are listed here.
                </li>
              )}
              {snap?.command_log.map((l, i) => (
                <li key={i}>{l}</li>
              ))}
              {snap?.project && snap.project.undo_history.length > 0 && (
                <li className="muted">
                  Undo stack (oldest first): {snap.project.undo_history.join(" → ")}
                </li>
              )}
            </ul>
          )}
        </div>
      )}
    </div>
  );
}
