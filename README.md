# agent-jsonl-compact

OpenAI Codex CLI rollout JSONL と Anthropic Claude Code transcript JSONL を自動判定し、会話・途中過程だけを compact JSONL、Markdown、summary JSON に抽出する Rust CLI です。

移植元は、提供リポジトリ内の次の Python スクリプトです。

```text
skills/session-transcript-extractor/scripts/extract_session_jsonl.py
```

## Build

```bash
cargo test
cargo build --release
```

生成されるバイナリは次です。

```bash
target/release/agent-jsonl-compact
```

単一バイナリとして配置する場合は、このファイルを任意の PATH 配下へコピーしてください。

```bash
install -m 0755 target/release/agent-jsonl-compact ~/.local/bin/agent-jsonl-compact
```

Linux で libc 依存を避けたい場合は、環境に musl target を追加したうえで次を実行します。

```bash
rustup target add x86_64-unknown-linux-musl
cargo build --release --target x86_64-unknown-linux-musl
```

## Usage

```bash
agent-jsonl-compact -i <path/to/session.jsonl> -o <out_dir>

agent-jsonl-compact -i <path/to/session.jsonl> --stats

agent-jsonl-compact \
  -i ~/.codex/sessions/2026/06/07/rollout-xxx.jsonl \
  -o temp/session_extracts \
  --msg-chars 4000 \
  --out-chars 2000
```

既定値は忠実性優先です。`--msg-chars 0` と `--out-chars 0` は全文保持を意味します。サイズをさらに削る場合のみ、正の値を指定してください。

## Outputs

`<name>` は入力ファイル名の stem、または `--name` で指定した値です。

```text
<name>.clean.jsonl
<name>.transcript.md
<name>.summary.json
```

`--format-out jsonl` または `--format-out md` を指定した場合でも、summary JSON は常に出力します。

## Main options

```text
--format auto|codex|claude_code
--channel terminal|api|both      # Codex のみ有効
--msg-chars N                    # 0 は全文保持
--out-chars N                    # 0 は全文保持
--elide-outputs                  # 肥大ツール出力を件数と先頭行へ畳む
--keep-token-count               # Codex token_count を保持
--no-dedup                       # 重複畳み込みを無効化
--format-out jsonl|md|both
--stats
```

## Tasks (justfile)

```bash
just build      # release バイナリをビルド
just demo       # build → 同梱の合成 fixture で利用例を実行(./demo-out に3種出力)
just stats <f>  # 形式とレコード型分布のみ表示
just test       # 全テスト
just check      # fmt-check + clippy(-D warnings) + test
just install    # ~/.local/bin へインストール
```

## S/N (signal vs noise) policy

既定は「真のノイズだけを削り、シグナルは削らない」忠実モードです。

- 削る(純ノイズ): Codex `token_count`(テレメトリ) / 注入 system 命令 / 冗長チャンネル /
  Claude Code の `ai-title`/`mode`/`attachment` 等 / 連続同一の重複。
- 残す(シグナル): 全メッセージ全文(`--msg-chars 0`)・全ツール出力(`--out-chars 0`)・
  逐次 commentary・全ツール呼出。
- サイズ優先が必要なときだけ lossy ノブ(`--msg-chars N`/`--out-chars N`/`--elide-outputs`/
  `--channel api`)を明示的に足す。

## Safety / PII

実セッションの JSONL とその抽出物(`*.clean.jsonl`/`*.transcript.md`)は会話本文・ホームパス・
プロジェクト内容(PII)を含む。**抽出物はコミットしない**(`.gitignore` 済)。同梱 fixture は
合成データのみ。

## pyo3 (future / optional)

基本はバイナリ運用。Python から直接呼びたい場合に備え、将来 `pyo3` で `lib.rs` の薄ラッパーを
公開する余地を残してある(本バージョンでは未提供)。当面は CLI バイナリを subprocess 経由で利用する。

## Development layout

```text
src/cli.rs       CLI option 定義
src/detect.rs    Codex / Claude Code 自動判定
src/classify.rs  生 JSON レコードから正規化イベントへの展開
src/clean.rs     keep set、truncate、dedup、elide
src/render.rs    Markdown レンダリング
src/runner.rs    集計、summary、ファイル出力
src/util.rs      JSONL streaming と JSON helper
```

