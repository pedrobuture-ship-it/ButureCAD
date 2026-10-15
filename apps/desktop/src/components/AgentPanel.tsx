import { useState } from "react";
import { api } from "../api";
import { call } from "../actions";
import type { AgentDeltaInfo } from "../bindings/AgentDeltaInfo";
import { useUi } from "../store";

const SIGN: Record<string, string> = { added: "+", removed: "−", modified: "~" };

function Delta({ label, d }: { label: string; d: AgentDeltaInfo | null }) {
  if (!d) return null;
  const worse = d.errors_after > d.errors_before;
  return (
    <div>
      <strong>{label}</strong>: errors {d.errors_before} → {d.errors_after}, warnings{" "}
      {d.warnings_before} → {d.warnings_after}
      {d.resolved > 0 ? `, ${d.resolved} resolved` : ""}
      {worse && <span className="warn"> — would add errors</span>}
      {d.new.length > 0 && (
        <ul>
          {d.new.map((n) => (
            <li key={n} className={n.startsWith("error") ? "warn" : "muted"}>
              {n}
            </li>
          ))}
        </ul>
      )}
    </div>
  );
}

/**
 * AI Workspace. There is no model in ButureCAD: a provider turns a request into a proposal, the
 * core validates it on a copy and shows the diff and the ERC/DRC comparison, and only Apply (one
 * undo step) or Reject changes anything.
 */
export function AgentPanel() {
  const info = useUi((s) => s.snapshot?.agent);
  const [text, setText] = useState("Add decoupling to U1");
  const [inspect, setInspect] = useState(false);
  if (!info) return null;
  const cmd = (n: string, a?: string) => void call(api.agentCommand(n, a));
  const sel = info.proposals.find((p) => p.id === info.selected) ?? info.proposals[0];
  const provider = info.providers.find((p) => p.id === info.provider);
  return (
    <div className="libmgr">
      <div className="symbol-bar">
        <label>
          Provider
          <select value={info.provider} onChange={(e) => cmd("provider", e.target.value)}>
            {info.providers.map((p) => (
              <option key={p.id} value={p.id}>
                {p.name} [{p.kind}]
              </option>
            ))}
          </select>
        </label>
        <span className="muted">
          {provider?.description} No language model is connected; a model would be another provider.
        </span>
      </div>
      <div className="symbol-bar" style={{ alignItems: "flex-start" }}>
        <textarea
          aria-label="Request"
          rows={provider?.id === "json" ? 6 : 2}
          style={{ flex: 1, minWidth: 280, font: "inherit" }}
          value={text}
          onChange={(e) => setText(e.target.value)}
          placeholder={
            provider?.id === "json" ? '{"title": "…", "actions": [ … ]}' : "Add decoupling to U1"
          }
        />
        <button className="primary" onClick={() => cmd("request", text)}>
          Propose
        </button>
      </div>
      {info.error && (
        <div className="warn" role="alert">
          {info.error}
        </div>
      )}
      <div className="libmgr-body">
        <ul className="symbol-list" role="listbox" aria-label="Proposals">
          {info.proposals.length === 0 && <li className="muted">No proposals yet.</li>}
          {info.proposals.map((p) => (
            <li
              key={p.id}
              role="option"
              aria-selected={sel?.id === p.id}
              className={sel?.id === p.id ? "sel" : ""}
              onClick={() => cmd("select", p.id)}
            >
              <strong>
                {p.id} · {p.title}
              </strong>
              <span className="muted">
                {p.status}
                {p.stale ? " (stale)" : ""} · from {p.origin}
              </span>
            </li>
          ))}
        </ul>
        <div className="libmgr-detail">
          {!sel && <p className="muted">Select or create a proposal to review it.</p>}
          {sel && (
            <>
              <h3>
                {sel.id} · {sel.title}
              </h3>
              {sel.rationale.length > 0 && (
                <ul>
                  {sel.rationale.map((r) => (
                    <li key={r}>{r}</li>
                  ))}
                </ul>
              )}
              {sel.failed_action && (
                <p className="warn" role="alert">
                  Invalid — {sel.failed_action}. Nothing was changed.
                </p>
              )}
              {sel.stale && (
                <p className="warn">The project changed after this was validated. Propose again.</p>
              )}
              {sel.warnings.map((w) => (
                <p key={w} className="warn">
                  {w}
                </p>
              ))}
              <h4>Actions</h4>
              <ol>
                {sel.actions.map((a, i) => (
                  <li key={i}>{a}</li>
                ))}
              </ol>
              {sel.diff.length > 0 && (
                <>
                  <h4>Diff ({sel.diff.length})</h4>
                  <ul className="agent-diff">
                    {sel.diff.map((d, i) => (
                      <li key={i} className={`diff-${d.change}`}>
                        <code>{SIGN[d.change]}</code> [{d.domain}] {d.kind} {d.label}
                      </li>
                    ))}
                  </ul>
                </>
              )}
              <h4>Checks on the proposed result</h4>
              <Delta label="ERC" d={sel.erc} />
              <Delta label="DRC" d={sel.drc} />
              <div className="symbol-bar">
                <button
                  className="primary"
                  disabled={sel.status !== "pending" || sel.stale}
                  onClick={() => cmd("apply", sel.id)}
                  title="Apply as one undoable step"
                >
                  Apply
                </button>
                <button
                  disabled={sel.status === "applied" || sel.status === "rejected"}
                  onClick={() => cmd("reject", sel.id)}
                >
                  Reject
                </button>
                <button onClick={() => setInspect(!inspect)}>Inspect</button>
              </div>
              {inspect && <pre className="agent-json">{sel.json}</pre>}
            </>
          )}
        </div>
      </div>
      <details style={{ padding: 8 }}>
        <summary className="muted">Audit log ({info.audit.length} most recent)</summary>
        <ul>
          {info.audit.map((a, i) => (
            <li key={i} className="muted">
              <code>{a}</code>
            </li>
          ))}
        </ul>
      </details>
    </div>
  );
}
