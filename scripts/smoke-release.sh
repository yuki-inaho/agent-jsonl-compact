#!/usr/bin/env bash
# Smoke-test a built release binary against the sanitized real-transcript fixture.
# Usage: scripts/smoke-release.sh <path/to/agent-jsonl-compact> [fixture]
set -euo pipefail

bin="${1:?usage: smoke-release.sh <binary> [fixture]}"
fixture="${2:-tests/fixtures/claude_desktop_sample.jsonl}"
out="$(mktemp -d)"
trap 'rm -rf "$out"' EXIT

test -f "$fixture"

"$bin" --version
"$bin" version --output json | grep -q '"version"'
"$bin" -i "$fixture" --stats >/dev/null
"$bin" view -i "$fixture" --tail 3 --output jsonl >/dev/null
"$bin" search -i "$fixture" --query jsonl --context 1 --output jsonl >/dev/null

# Default extraction must produce clean JSONL.
"$bin" -i "$fixture" -o "$out/default" >/dev/null
test -s "$out/default/claude_desktop_sample.clean.jsonl"

# Explicit transcript output.
"$bin" -i "$fixture" -o "$out/all" --format-out both >/dev/null
test -s "$out/all/claude_desktop_sample.clean.jsonl"
test -s "$out/all/claude_desktop_sample.transcript.md"

echo "smoke OK: $bin"
