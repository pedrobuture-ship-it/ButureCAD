import { useSyncExternalStore } from "react";
import type { PointerResult } from "./bindings/PointerResult";
import type { Snapshot } from "./bindings/Snapshot";

export type BottomTab = "erc" | "drc" | "messages" | "console";

export type Dialog =
  | { kind: "new-project" }
  | { kind: "save-as" }
  | { kind: "symbol-picker"; power: boolean }
  | { kind: "label-name"; global: boolean }
  | { kind: "rename-project" }
  | { kind: "unsaved"; then: () => Promise<void>; title: string }
  | { kind: "recovery" }
  | { kind: "about" };

export interface ContextMenuState {
  x: number;
  y: number;
}

export interface UiState {
  snapshot: Snapshot | null;
  pointer: PointerResult | null;
  bottomTab: BottomTab;
  bottomOpen: boolean;
  inspectorOpen: boolean;
  paletteOpen: boolean;
  contextMenu: ContextMenuState | null;
  dialog: Dialog | null;
  toast: string | null;
  /** Latest status-bar hint (from the newest snapshot or pointer result). */
  hint: string;
  /** Bumped to ask the canvas for a new frame. */
  renderTick: number;
}

const initial: UiState = {
  snapshot: null,
  pointer: null,
  bottomTab: "messages",
  bottomOpen: true,
  inspectorOpen: true,
  paletteOpen: false,
  contextMenu: null,
  dialog: null,
  toast: null,
  hint: "",
  renderTick: 0,
};

type Listener = () => void;

class Store {
  private state: UiState = initial;
  private listeners = new Set<Listener>();

  getState = (): UiState => this.state;

  subscribe = (l: Listener): (() => void) => {
    this.listeners.add(l);
    return () => this.listeners.delete(l);
  };

  set(patch: Partial<UiState>): void {
    this.state = { ...this.state, ...patch };
    this.listeners.forEach((l) => l());
  }

  /** Test helper. */
  reset(): void {
    this.state = initial;
  }
}

export const store = new Store();

export function useUi<T>(selector: (s: UiState) => T): T {
  return useSyncExternalStore(store.subscribe, () => selector(store.getState()));
}
