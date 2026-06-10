# agent-jsonl-compact — task runner
# Codex CLI rollout / Claude Code transcript JSONL を軽量化する単体 Rust CLI。

# 一覧表示
default:
    @just --list

# release バイナリをビルド
build:
    cargo build --release

# 全テスト(unit + integration)
test:
    cargo test

# フォーマット
fmt:
    cargo fmt

fmt-check:
    cargo fmt --check

# lint(警告はエラー扱い)
lint:
    cargo clippy --all-targets -- -D warnings

# 品質ゲート一括
check: fmt-check lint test

# ビルド→同梱の合成 fixture で利用例を実行(./demo-out に3種出力)。build から利用例までの導線。
demo: build
    @rm -rf demo-out
    ./target/release/agent-jsonl-compact -i tests/fixtures/codex_sample.jsonl  -o demo-out
    ./target/release/agent-jsonl-compact -i tests/fixtures/claude_sample.jsonl -o demo-out
    @echo "=== demo-out ===" && ls -la demo-out

# 形式とレコード型分布のみ表示(抽出はしない)
stats input:
    cargo run --release -- -i "{{input}}" --stats

# ~/.local/bin へインストール
install: build
    install -m 0755 target/release/agent-jsonl-compact ~/.local/bin/agent-jsonl-compact
    @echo "installed: ~/.local/bin/agent-jsonl-compact (ensure ~/.local/bin is on PATH)"
