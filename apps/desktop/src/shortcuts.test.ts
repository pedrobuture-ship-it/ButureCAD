import { describe, expect, it } from "vitest";
import { matchesShortcut, parseShortcut, type KeyLike } from "./shortcuts";

const key = (k: string, mods: Partial<KeyLike> = {}): KeyLike => ({
  key: k,
  ctrlKey: false,
  shiftKey: false,
  altKey: false,
  metaKey: false,
  ...mods,
});

describe("shortcuts", () => {
  it("parses modifiers and aliases", () => {
    expect(parseShortcut("Ctrl+Shift+P")).toEqual({
      key: "p",
      ctrl: true,
      shift: true,
      alt: false,
    });
    expect(parseShortcut("Esc").key).toBe("escape");
    expect(parseShortcut("Ctrl++")).toEqual({ key: "+", ctrl: true, shift: false, alt: false });
    expect(parseShortcut("Ctrl+-").key).toBe("-");
  });

  it("matches exactly the requested modifiers", () => {
    const save = parseShortcut("Ctrl+S");
    expect(matchesShortcut(key("s", { ctrlKey: true }), save)).toBe(true);
    expect(matchesShortcut(key("S", { ctrlKey: true }), save)).toBe(true);
    expect(matchesShortcut(key("s"), save)).toBe(false);
    expect(matchesShortcut(key("s", { ctrlKey: true, shiftKey: true }), save)).toBe(false);
    expect(matchesShortcut(key("s", { ctrlKey: true, altKey: true }), save)).toBe(false);
    expect(matchesShortcut(key("s", { metaKey: true }), save)).toBe(true);
  });

  it("distinguishes Ctrl+Shift+S from Ctrl+S", () => {
    const saveAs = parseShortcut("Ctrl+Shift+S");
    expect(matchesShortcut(key("S", { ctrlKey: true, shiftKey: true }), saveAs)).toBe(true);
    expect(matchesShortcut(key("s", { ctrlKey: true }), saveAs)).toBe(false);
  });

  it("matches symbol keys regardless of how Shift produced them", () => {
    const zoomIn = parseShortcut("Ctrl++");
    expect(matchesShortcut(key("+", { ctrlKey: true, shiftKey: true }), zoomIn)).toBe(true);
    expect(matchesShortcut(key("+", { ctrlKey: true }), zoomIn)).toBe(true);
    expect(matchesShortcut(key("-", { ctrlKey: true }), zoomIn)).toBe(false);
  });

  it("matches named keys", () => {
    expect(matchesShortcut(key("Escape"), parseShortcut("Esc"))).toBe(true);
    expect(matchesShortcut(key("Delete"), parseShortcut("Del"))).toBe(true);
    expect(matchesShortcut(key("F3"), parseShortcut("F3"))).toBe(true);
  });
});
