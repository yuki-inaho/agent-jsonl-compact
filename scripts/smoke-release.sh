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
"$bin" version --output json | grep -q '"source_commit"'
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

# F-01: destination boundaries on the actual release binary.
simple="${3:-tests/fixtures/codex_sample.jsonl}"
mkdir -p "$out/link"
victim="$out/victim"
printf 'keep\n' > "$victim"
ln -s "$victim" "$out/link/codex_sample.clean.jsonl"
if "$bin" -i "$simple" -o "$out/link" >/dev/null 2>&1; then
  echo "release boundary check failed: symlink destination was accepted" >&2
  exit 1
fi
test "$(cat "$victim")" = keep
if "$bin" -i "$simple" -o "$out/traversal" --name ../escape >/dev/null 2>&1; then
  echo "release boundary check failed: path traversal was accepted" >&2
  exit 1
fi
cp "$simple" "$out/alias.clean.jsonl"
if "$bin" -i "$out/alias.clean.jsonl" -o "$out" --name alias >/dev/null 2>&1; then
  echo "release boundary check failed: input replacement was accepted" >&2
  exit 1
fi
"$bin" -i "$simple" -o "$out/perm" >/dev/null
if mode="$(stat -c %a "$out/perm/codex_sample.clean.jsonl" 2>/dev/null)"; then :; else
  mode="$(stat -f %Lp "$out/perm/codex_sample.clean.jsonl")"
fi
test "$mode" = 600

echo "smoke OK: $bin (destination boundaries and 0600 verified)"
