/** Keyboard shortcut parsing/matching. Pure, no DOM dependency (tested without a browser). */

export interface Shortcut {
  key: string;
  ctrl: boolean;
  shift: boolean;
  alt: boolean;
}

export interface KeyLike {
  key: string;
  ctrlKey: boolean;
  shiftKey: boolean;
  altKey: boolean;
  metaKey: boolean;
}

const ALIASES: Record<string, string> = {
  esc: "escape",
  del: "delete",
  plus: "+",
  minus: "-",
  space: " ",
};

export function parseShortcut(text: string): Shortcut {
  const parts = text.split("+").map((p) => p.trim());
  // "Ctrl++" splits into [..., "", ""]: the key is a literal "+".
  const key = text.endsWith("++") ? "+" : (parts[parts.length - 1] ?? "");
  const mods = new Set(parts.slice(0, -1).map((p) => p.toLowerCase()));
  const k = key.toLowerCase();
  return {
    key: ALIASES[k] ?? k,
    ctrl: mods.has("ctrl") || mods.has("cmd"),
    shift: mods.has("shift"),
    alt: mods.has("alt"),
  };
}

export function matchesShortcut(e: KeyLike, shortcut: Shortcut): boolean {
  const key = e.key.toLowerCase();
  const ctrl = e.ctrlKey || e.metaKey;
  if (ctrl !== shortcut.ctrl || e.altKey !== shortcut.alt) return false;
  // Shifted symbols (e.g. "+" is Shift+"=") arrive already shifted in `key`; only enforce Shift
  // for letter/named keys.
  const isSymbol = key.length === 1 && !/[a-z0-9]/.test(key);
  if (!isSymbol && e.shiftKey !== shortcut.shift) return false;
  return key === shortcut.key;
}

export function formatShortcut(text: string): string {
  return text.replace(/\bEscape\b/i, "Esc");
}
