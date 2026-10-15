#!/usr/bin/env bash
# Runs the headless benchmark for every size (one process per size) and writes
# benchmarks/results/<label>-headless.jsonl and a Markdown table.
#   benchmarks/run.sh <label> [sizes...]        e.g.  benchmarks/run.sh before 100 500 2000 5000
set -euo pipefail
cd "$(dirname "$0")/.."
label="${1:?label (e.g. before / after)}"; shift || true
sizes=("$@"); [ ${#sizes[@]} -eq 0 ] && sizes=(100 500 2000 5000)
cargo build --release -q -p buturecad-cli --example bench
out="benchmarks/results/${label}-headless.jsonl"; : > "$out"
for n in "${sizes[@]}"; do
  echo "== $n components" >&2
  timeout 1800 target/release/examples/bench "$n" >> "$out" || echo "size $n did not finish (timeout or error)" >&2
done
python3 benchmarks/table.py "$out" > "benchmarks/results/${label}-headless.md"
cat "benchmarks/results/${label}-headless.md"
