import type { Mode } from "./../bindings/Mode";
import { MODE_PLANNED, MODE_TITLES } from "../tools";
import { useUi } from "../store";
import { useActions } from "./hooks";

const ORDER: Mode[] = ["schematic", "pcb", "symbol_editor", "footprint_editor", "viewer3d"];

export function DocTabs() {
  const snap = useUi((s) => s.snapshot);
  const actions = useActions();
  return (
    <div className="doctabs" role="tablist">
      {ORDER.map((m) => {
        const real = !MODE_PLANNED[m];
        return (
          <button
            key={m}
            role="tab"
            aria-selected={snap?.mode === m}
            className={`doctab${snap?.mode === m ? " active" : ""}${real ? "" : " planned"}`}
            title={
              real ? MODE_TITLES[m] : `${MODE_TITLES[m]} — not implemented yet (${MODE_PLANNED[m]})`
            }
            onClick={() => actions.setMode(m)}
          >
            {MODE_TITLES[m]}
          </button>
        );
      })}
    </div>
  );
}
