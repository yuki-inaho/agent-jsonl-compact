#!/usr/bin/env bash
set -euo pipefail

cargo test
cargo build --release

echo "built: target/release/session-jsonl-compact"
