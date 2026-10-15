import { useEffect, useMemo, useRef, useState } from "react";
import { availability, filterCommands, runCommand } from "../commands";
import { commandHistory } from "../history";
import { formatShortcut } from "../shortcuts";
import { store, useUi } from "../store";
import { useActions, useCommands } from "./hooks";

export function CommandPalette() {
  const open = useUi((s) => s.paletteOpen);
  const state = useUi((s) => s);
  const commands = useCommands();
  const actions = useActions();
  const [query, setQuery] = useState("");
  const [index, setIndex] = useState(0);
  const inputRef = useRef<HTMLInputElement>(null);

  useEffect(() => {
    if (open) {
      setQuery("");
      setIndex(0);
      inputRef.current?.focus();
    }
  }, [open]);

  // Re-read the history each time the palette opens so the newest commands lead.
  const recent = useMemo(() => (open ? [...commandHistory.recent()] : []), [open]);
  const results = useMemo(() => filterCommands(commands, query, recent), [commands, query, recent]);
  const showRecent = query.trim() === "";
  if (!open) return null;

  const close = () => store.set({ paletteOpen: false });
  const run = (i: number) => {
    const c = results[i];
    if (!c || availability(c, state) !== true) return;
    close();
    runCommand(c, actions);
  };

  return (
    <div className="overlay" onMouseDown={close}>
      <div
        className="palette"
        role="dialog"
        aria-label="Command palette"
        onMouseDown={(e) => e.stopPropagation()}
      >
        <input
          ref={inputRef}
          value={query}
          placeholder="Type a command…"
          onChange={(e) => {
            setQuery(e.target.value);
            setIndex(0);
          }}
          onKeyDown={(e) => {
            if (e.key === "ArrowDown") {
              e.preventDefault();
              setIndex((i) => Math.min(results.length - 1, i + 1));
            } else if (e.key === "ArrowUp") {
              e.preventDefault();
              setIndex((i) => Math.max(0, i - 1));
            } else if (e.key === "Enter") {
              e.preventDefault();
              run(index);
            } else if (e.key === "Escape") {
              close();
            }
          }}
        />
        <ul role="listbox">
          {results.length === 0 && <li className="muted pal-empty">No matching command</li>}
          {results.map((c, i) => {
            const a = availability(c, state);
            return (
              <li
                key={c.id}
                role="option"
                aria-selected={i === index}
                aria-disabled={a !== true}
                className={`${i === index ? "sel" : ""}${a === true ? "" : " disabled"}`}
                onMouseEnter={() => setIndex(i)}
                onClick={() => run(i)}
                title={a === true ? undefined : a}
              >
                <span className="pal-cat">
                  {showRecent && recent.includes(c.id) ? "Recent" : c.category}
                </span>
                <span className="pal-title">{c.title}</span>
                {a !== true && <span className="pal-why">{a}</span>}
                {c.shortcut && <kbd>{formatShortcut(c.shortcut)}</kbd>}
              </li>
            );
          })}
        </ul>
      </div>
    </div>
  );
}
