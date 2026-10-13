import type { Mode } from "../bindings/Mode";
import { MODE_PLANNED, MODE_TITLES } from "../tools";

export function NotImplemented({ mode }: { mode: Mode }) {
  return (
    <div className="start">
      <h1>{MODE_TITLES[mode]}</h1>
      <p>This editor is not implemented yet.</p>
      <p className="muted">
        Planned in {MODE_PLANNED[mode] ?? "a later roadmap stage"}. Nothing here is a mock-up:
        switching modes never changes your project.
      </p>
    </div>
  );
}
