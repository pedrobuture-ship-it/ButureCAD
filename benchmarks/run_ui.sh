#!/usr/bin/env bash
# GUI-side benchmark (rendering, zoom, pan, selection) for every size; one process per size.
#   benchmarks/run_ui.sh <label> [sizes...]
set -euo pipefail
cd "$(dirname "$0")/.."
label="${1:?label}"; shift || true
sizes=("$@"); [ ${#sizes[@]} -eq 0 ] && sizes=(100 500 2000 5000)
cargo build --release -q -p buturecad-desktop --example bench_ui
out="benchmarks/results/${label}-ui.jsonl"; : > "$out"
for n in "${sizes[@]}"; do
  echo "== $n components" >&2
  timeout 1800 target/release/examples/bench_ui "$n" >> "$out" 2>/dev/null || echo "size $n did not finish" >&2
done
python3 benchmarks/table.py "$out" --ui > "benchmarks/results/${label}-ui.md"
cat "benchmarks/results/${label}-ui.md"
