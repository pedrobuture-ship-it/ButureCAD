#!/usr/bin/env python3
"""Turns a benchmark .jsonl into a Markdown table (phases as rows, sizes as columns)."""
import json, sys
ui = "--ui" in sys.argv
rows, sizes, order = {}, [], []
for line in open(sys.argv[1]):
    line = line.strip()
    if not line.startswith("{"):
        continue
    r = json.loads(line)
    if r["size"] not in sizes:
        sizes.append(r["size"])
    if r["phase"] not in order:
        order.append(r["phase"])
    rows[(r["phase"], r["size"])] = r
def fmt(ms):
    return f"{ms/1000:.2f} s" if ms >= 1000 else f"{ms:.1f} ms"
print("| phase (median" + (" latency; FPS" if ui else " wall time") + ") | " + " | ".join(f"{s} comp." for s in sizes) + " |")
print("|---|" + "---:|" * len(sizes))
for p in order:
    def cell(r):
        if not ui or r.get("frames", 1) < 2:
            return fmt(r["ms"])
        return f'{fmt(r["ms"])} (p95 {fmt(r["p95_ms"])}, {r["fps"]:.0f} fps)'
    print(f"| {p} | " + " | ".join(cell(rows[(p, s)]) if (p, s) in rows else "–" for s in sizes) + " |")
print("| **peak memory (RSS high-water)** | " + " | ".join(f"{max((rows[(p, s)]['peak_mb'] for p in order if (p, s) in rows), default=0):.0f} MB" for s in sizes) + " |")
