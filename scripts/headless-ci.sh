#!/usr/bin/env bash
# Headless ButureCAD pipeline for CI: ButureScript -> core -> schematic -> PCB -> ERC -> DRC -> Gerber.
# Needs only Rust: no GUI libraries, no display, no network. Usage: scripts/headless-ci.sh [script.beda]
set -euo pipefail
cd "$(dirname "$0")/.."
script="${1:-examples/chain.beda}"
work="$(mktemp -d)"
trap 'rm -rf "$work"' EXIT
cargo build -q -p buturecad-cli
bin=target/debug/buturecad
"$bin" check "$script"
"$bin" build "$script" --out "$work/project"
"$bin" check "$work/project"
"$bin" erc "$work/project"
"$bin" drc "$work/project"
"$bin" export-gerber "$work/project" --out "$work/production.zip"
"$bin" export-bom "$work/project"
"$bin" inspect board "$work/project"
echo "headless pipeline OK ($(unzip -l "$work/production.zip" 2>/dev/null | tail -1 || echo zip written))"
