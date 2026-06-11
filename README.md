# agent-jsonl-compact

[![ci](https://github.com/yuki-inaho/agent-jsonl-compact/actions/workflows/ci.yml/badge.svg)](https://github.com/yuki-inaho/agent-jsonl-compact/actions/workflows/ci.yml)

OpenAI Codex CLI rollout JSONL と Anthropic Claude Code transcript JSONL を自動判定し、
会話・思考・ツール実行だけを compact JSONL / Markdown / summary JSON に抽出する単一 Rust CLI です。
巨大なセッションログを「軽量化してから段階的に読む」ための CLI と、それを使うエージェントスキル
(`agent-jsonl-compact-reader`)をまとめて提供します。

移植元は次の Python スクリプトです(別リポジトリ)。

```text
skills/session-transcript-extractor/scripts/extract_session_jsonl.py
```

## Install (prebuilt)

Rust/cargo 不要。GitHub Releases の Linux x86_64 musl 静的バイナリを `~/.local/bin` へ入れます。

```bash
curl -fsSL https://raw.githubusercontent.com/yuki-inaho/agent-jsonl-compact/main/install.sh | bash
```

バージョン固定やインストール先変更は、パイプ先の `bash` へ環境変数を渡します。

```bash
curl -fsSL https://raw.githubusercontent.com/yuki-inaho/agent-jsonl-compact/main/install.sh \
  | AJC_VERSION=v0.1.0 INSTALL_DIR=~/bin bash
```

| 環境変数 | 意味 | 既定 |
|---|---|---|
| `AJC_VERSION` | 取得する release タグ | 最新 release |
| `INSTALL_DIR` | インストール先 | `~/.local/bin` |

対応プラットフォームは Linux x86_64(musl)のみです。tarball は sha256 で検証します。
他環境は下記 [Build from source](#build-from-source) を使ってください。

## Skill: agent-jsonl-compact-reader

既存セッション JSONL を「軽量化してから段階的に読む」ための Claude Code / Codex 用スキルです。
`summary.json` で全体規模を掴み、必要箇所だけ `transcript.md` / `clean.jsonl` を読むことで、
巨大ログを生のままコンテキストへ載せずに把握します。SKILL.md はバイナリに埋め込まれています。

バイナリ導入後、**CLI 自身がスキルを各エージェントへ配置**します
(Microsoft `playwright-cli install --skills` と同じ方式)。

```bash
agent-jsonl-compact install-skills              # ~/.claude と ~/.codex の両方(存在する側のみ)
agent-jsonl-compact install-skills --claude-only
agent-jsonl-compact install-skills --codex-only
```

配置先:

```text
~/.claude/skills/agent-jsonl-compact-reader/SKILL.md
~/.codex/skills/agent-jsonl-compact-reader/SKILL.md
```

prebuilt と合わせればワンライナーです。

```bash
curl -fsSL https://raw.githubusercontent.com/yuki-inaho/agent-jsonl-compact/main/install.sh | bash \
  && ~/.local/bin/agent-jsonl-compact install-skills
```

開発中にリポジトリの SKILL.md を直接編集しながら使う場合は symlink でも登録できます。

```bash
ln -sfn "$PWD/skills/agent-jsonl-compact-reader" ~/.claude/skills/agent-jsonl-compact-reader
ln -sfn "$PWD/skills/agent-jsonl-compact-reader" ~/.codex/skills/agent-jsonl-compact-reader
```

## Usage

抽出(既定動作):

```bash
agent-jsonl-compact -i <path/to/session.jsonl> -o <out_dir>

agent-jsonl-compact -i <path/to/session.jsonl> --stats

agent-jsonl-compact \
  -i ~/.codex/sessions/2026/06/07/rollout-xxx.jsonl \
  -o temp/session_extracts \
  --msg-chars 4000 \
  --out-chars 2000
```

サブコマンド / 情報:

```bash
agent-jsonl-compact install-skills [--claude-only|--codex-only]
agent-jsonl-compact --version
agent-jsonl-compact --help
```

既定値は忠実性優先です。`--msg-chars 0` と `--out-chars 0` は全文保持を意味します。
サイズをさらに削る場合のみ、正の値を指定してください。

## Outputs

`<name>` は入力ファイル名の stem、または `--name` で指定した値です。

```text
<name>.clean.jsonl
<name>.transcript.md
<name>.summary.json
```

`--format-out jsonl` または `--format-out md` を指定した場合でも、summary JSON は常に出力します。

## Options (抽出)

```text
--format auto|codex|claude_code
--channel terminal|api|both      # Codex のみ有効
--msg-chars N                    # 0 は全文保持
--out-chars N                    # 0 は全文保持
--elide-outputs                  # 肥大ツール出力を件数と先頭行へ畳む
--keep-token-count               # Codex token_count を保持
--no-dedup                       # 重複畳み込みを無効化
--format-out jsonl|md|both
--stats                          # 形式とレコード型分布のみ表示して終了
--name <stem>                    # 出力ベース名(既定は入力 stem)
```

## Build from source

```bash
cargo test
cargo build --release            # -> target/release/agent-jsonl-compact
just install                     # ローカルビルドを ~/.local/bin へ
```

配布と同じ musl 静的バイナリ・tarball をローカルで再現するには:

```bash
just build-musl                  # musl 静的バイナリ
just dist                        # tarball + sha256 を dist/ に生成(CI と同等)
```

## Tasks (justfile)

```bash
just                 # タスク一覧(= just --list)
just build           # release バイナリをビルド
just test            # 全テスト(unit + integration)
just check           # fmt-check + clippy(-D warnings) + test(= CI と同じゲート)
just demo            # build → 同梱 fixture で利用例を実行(./demo-out に3種出力)
just stats <f>       # 形式とレコード型分布のみ表示
just build-musl      # musl 静的バイナリをビルド(配布用)
just dist            # 配布 tarball + sha256 を dist/ に生成
just install         # ローカルビルドを ~/.local/bin へ
just install-release # 手元の install.sh で prebuilt を ~/.local/bin へ
just install-skills  # reader スキルを各エージェントへ配置
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
src/main.rs       エントリポイント(run_cli を呼ぶだけ)
src/lib.rs        モジュール公開・run_cli・公開 API(run / install_skills_into)
src/cli.rs        CLI option と install-skills サブコマンド定義
src/detect.rs     Codex / Claude Code 自動判定
src/classify.rs   生 JSON レコードから正規化イベントへの展開
src/clean.rs      keep set、truncate、dedup、elide
src/render.rs     Markdown レンダリング
src/counter.rs    生レコード型・正規化 kind の集計
src/runner.rs     実行制御・集計・summary・ファイル出力・install-skills 実装
src/util.rs       JSONL streaming と JSON helper

skills/agent-jsonl-compact-reader/SKILL.md  reader スキル(ビルド時にバイナリへ埋め込み)
.github/workflows/ci.yml       fmt-check + clippy + test
.github/workflows/release.yml  v* タグで musl tarball + sha256 を Releases へ
install.sh                     prebuilt インストーラ(sha256 検証)
```

> SKILL.md はバイナリに `include_str!` で埋め込まれます。スキル本文を変更したら再ビルドが必要です。
```
