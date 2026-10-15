/**
 * Command history: which commands the user ran lately. Feeds "Repeat Last Command", "Previous
 * Tool" and the recent-first ordering of the command palette.
 *
 * Pure (no DOM): the storage is injected so it is tested without a browser. It only remembers
 * command ids, per viewer; nothing leaves the machine.
 */

const KEY = "buturecad.commandHistory.v1";
const MAX = 40;

export interface KeyValueStorage {
  getItem(key: string): string | null;
  setItem(key: string, value: string): void;
}

function browserStorage(): KeyValueStorage | null {
  try {
    return typeof localStorage === "undefined" ? null : localStorage;
  } catch {
    return null; // blocked storage (private window, policy): the history just stays in memory
  }
}

export class CommandHistory {
  /** Most recent first, no duplicates. */
  private ids: string[] = [];

  constructor(private readonly storage: KeyValueStorage | null = browserStorage()) {
    try {
      const raw = storage?.getItem(KEY);
      const parsed: unknown = raw ? JSON.parse(raw) : [];
      if (Array.isArray(parsed)) {
        this.ids = parsed.filter((x): x is string => typeof x === "string").slice(0, MAX);
      }
    } catch {
      this.ids = []; // damaged value: start empty rather than fail
    }
  }

  record(id: string): void {
    this.ids = [id, ...this.ids.filter((x) => x !== id)].slice(0, MAX);
    try {
      this.storage?.setItem(KEY, JSON.stringify(this.ids));
    } catch {
      /* quota or blocked storage: keep the in-memory history */
    }
  }

  /** Recent command ids, most recent first. */
  recent(): readonly string[] {
    return this.ids;
  }

  clear(): void {
    this.ids = [];
    try {
      this.storage?.setItem(KEY, "[]");
    } catch {
      /* see record() */
    }
  }
}

export const commandHistory = new CommandHistory();
