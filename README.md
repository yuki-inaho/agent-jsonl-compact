# agent-jsonl-compact

[![ci](https://github.com/yuki-inaho/agent-jsonl-compact/actions/workflows/ci.yml/badge.svg)](https://github.com/yuki-inaho/agent-jsonl-compact/actions/workflows/ci.yml)

OpenAI Codex CLI rollout JSONL、Anthropic Claude Code transcript JSONL、OpenCode
`run --format json` の NDJSON を自動判定し、
会話・思考・ツール実行だけを compact JSONL / Markdown / summary JSON に抽出する単一 Rust CLI です。
巨大なセッションログを「軽量化してから段階的に読む」ための CLI と、それを使うエージェントスキル
(`agent-jsonl-compact-reader`)をまとめて提供します。

0.3.0 では、既存の抽出をそのまま残したうえで、会話を探して必要箇所だけ読む
`sessions` / `view` / `search` を追加しました（[ADR 0001](docs/adr/0001-additive-session-viewer-cli.md)）。
抽出は「保存して読む」、閲覧系は「stdout に返して読む」という別の用途です。

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

OpenCode は通常、セッションを `~/.local/share/opencode/opencode.db` に保存し、JSONLを
自動作成しません。対応する入力は `run --format json` のstdoutを保存したものです。

```bash
opencode run --format json "<prompt>" > opencode-session.jsonl
agent-jsonl-compact -i opencode-session.jsonl -o temp/session_extracts
```

このストリームは `step_start` / `text` / `reasoning` / `tool_use` / `step_finish` /
`error` を含みます。ユーザープロンプトとモデル名は出力されないため、抽出器はそれらを
推測しません。`opencode export` は単一のJSON文書であり、このJSONL入力とは別形式です。

### 入力形式の一次資料と安定性

このCLIは3形式を扱いますが、公開された安定スキーマとして扱える範囲は同一ではありません。
各パーサーは未知フィールドを無視し、`--format` による明示指定を残して互換性リスクを局所化します。

| 形式 | 参照URL | このCLIでの位置付け |
|---|---|---|
| Codex CLI 0.152.0 | [公式 `recorder.rs`](https://github.com/openai/codex/blob/rust-v0.152.0/codex-rs/rollout/src/recorder.rs) / [公式互換テスト](https://github.com/openai/codex/blob/rust-v0.152.0/codex-rs/rollout/src/tests.rs) | ローカル導入版 `codex-cli 0.152.0` と同じタグに固定。writerがJSONLを1行ずつ書くこと、`timestamp` / `ordinal` / `type` / `payload` の実例を根拠にする実装由来形式。 |
| Claude Code | [公式 Sessions 文書](https://code.claude.com/docs/en/sessions#where-transcripts-are-stored) / [公式 `.claude` directory 文書](https://code.claude.com/docs/en/claude-directory) | JSONLの保存場所と「各行がmessage/tool/metadata object」であることは公式文書に基づく。一方、同文書は行内スキーマを内部形式・リリースごとに変更可能と明記するため、`sessionId` / `uuid` / `message.content` は対応確認済みのbest-effortとして扱う。 |
| OpenCode 1.18.26 | [CLI公式文書 — `run --format json`](https://dev.opencode.ai/docs/cli/#run) / [v1.18.26 `run.ts`](https://github.com/anomalyco/opencode/blob/v1.18.26/packages/opencode/src/cli/cmd/run.ts) / [SDK型定義](https://github.com/anomalyco/opencode/blob/v1.18.26/packages/sdk/js/src/v2/gen/types.gen.ts) / [session schema](https://github.com/anomalyco/opencode/blob/v1.18.26/packages/schema/src/v1/session.ts) | `run --format json` のstdout NDJSONを対象とし、型・実装を検証対象バージョンへ固定。 |

Codexは実装タグを更新したとき、Claude CodeはCLI更新後、OpenCodeは対応対象バージョンを更新したときに、
合成fixtureを追加して `just check` を通すことを変更の受入条件にします。

サブコマンド / 情報:

```bash
agent-jsonl-compact install-skills [--claude-only|--codex-only]
agent-jsonl-compact --version
agent-jsonl-compact version --output json
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
--format auto|codex|claude_code|opencode
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

## 閲覧CLI (0.3.0): 探す → 読む → 根拠へ戻る

既存の抽出が「ファイルへ保存する」のに対し、`sessions` / `view` / `search` は
**保存せず stdout へ返す**閲覧用のコマンドです。source は変更せず、cache や index も
既定では作りません。詳細な設計判断は
[ADR 0001](docs/adr/0001-additive-session-viewer-cli.md) /
[ADR 0002](docs/adr/0002-cursor-and-resource-boundaries.md) にあります。

最短経路:

```bash
agent-jsonl-compact sessions --provider codex --cwd /path/to/repo
agent-jsonl-compact view --session codex:SESSION_ID --tail 30 --output jsonl
agent-jsonl-compact search -i /path/rollout.jsonl --query keyword --context 2
```

### 版と更新日

```bash
agent-jsonl-compact --version            # semver のみ（従来と同じ）
agent-jsonl-compact version --output json
```

`version --output json` は `version`、固定の `source_release_date`、
`query_schema_version`、`cursor_version` を返します。更新日は **source の固定日**で、
build 時刻でも GitHub Release の更新時刻でもありません（[ADR 0003](docs/adr/0003-version-and-release-metadata.md)）。

### 1. 候補を探す — `sessions`

```bash
# cwd 完全一致（既定）。子ディレクトリも含めるなら --cwd-children
agent-jsonl-compact sessions --provider codex --cwd /path/to/repo --limit 20

# Claude Code も含める / 探索 root を明示する
agent-jsonl-compact sessions --provider all --root ~/.claude/projects --output jsonl
```

- 探索対象は provider ごとの既知 root と、`--root` で明示した root のみです。
  **OpenCode の run JSONL は既定 root では探しません**（`opencode.db` を推測しないため）。
  使うときは `--root` で明示してください。
- `--provider all` は codex / claude_code / opencode を対象にします。
- `--scan-limit`（既定 10000）に達した場合は `status: partial` として未走査を明示します。
- 表示は開始日時 (`started_at`) と最終 event 時刻 (`last_event_at`) を別々に返します。
  mtime は実行中・停止中の判定には使いません。

### 2. 必要箇所を読む — `view`

```bash
# 直近30件（filter 後の末尾、原文順）
agent-jsonl-compact view --session codex:SESSION_ID --tail 30

# ファイル指定 + 期間 + role
agent-jsonl-compact view -i /path/rollout.jsonl \
  --since 2026-10-21T00:00:00+09:00 --role user,assistant --output jsonl

# 1 event を再読（表示上限と text_complete は維持）
agent-jsonl-compact view -i /path/rollout.jsonl --event EVENT_ID --output jsonl
```

- 既定 scope は `messages` です。`--scope tools` で tool、`--scope all` でも
  private reasoning・system・developer・compaction は出ません。
- 既定の表示件数は 50 件、`--max-events`（primary + context + pair の総量）は 200 件です。
- `--with-pair` を付けると、選択した tool 呼出しに対応する結果を `call_id` で併記します。
  状態は `paired` / `pending_result`（走査完了後に不在を確認）/ `unknown_pair`（走査が
  不完全で未着と断定できない）/ `orphan`（呼出し無しの結果）/ `not_rendered_budget`
  （存在するが予算で未表示）の5つです。
- 対応件数は `paired_total`（実総数）と `paired_event_ids`（保持した ID）を分けて返し、
  打ち切り時は `paired_ids_truncated: true`、再入位置は `pair_reentry` で示します。
  配列の長さを相手の総数と読み替えないでください。`scan.with_pair.incomplete` は
  pair 走査自体が不完全だったことを示します。
- `text_complete: false` は「表示が cut された」意味で、取得範囲が不完全という意味ではありません。

### 3. 本文を検索する — `search`

```bash
agent-jsonl-compact search -i /path/rollout.jsonl \
  --query contour_v2 --query "エラー" --context 2 --limit 10

# 正規表現は明示したときだけ
agent-jsonl-compact search -i /path/rollout.jsonl --query 'timeout|retry' --regex
```

- `--query` の繰返しは **OR**。各ヒットは `matched_queries` を持ちます。
- `matched_ranges` は **原文 text に対する UTF-8 byte offset** です。表示が cut されていても
  位置は原文基準です。
- `--event EVENT_ID` は対象 event を絞るだけで、同じ予算のまま全文が返る保証はありません。
  全文が必要なら `--max-output-bytes`（必要なら `--max-record-bytes`）を上げて再実行するか、
  `source.byte_start` / `byte_end` で raw の該当範囲を直接読んでください。
- `--context K` は hard scope 内の前後 K event。role や時刻の soft filter を越えた隣接 event は
  `is_context: true` として明示されますが、reasoning / developer は決して含まれません。
- 同じ event がヒットかつ他ヒットの文脈である場合は、**primary として1回だけ**出ます。

### 4. 根拠と元位置

event 行は次を持ちます。

| field | 意味 |
|---|---|
| `event_id` | source 同一性 + raw 開始位置 + subevent から作る安定 ID |
| `source.raw_line` | raw 行番号（1 始まり） |
| `source.byte_start` / `byte_end` | raw byte 範囲（0 始まり・終端排他） |
| `source.subevent_index` | 1 raw が複数 event になる場合の index |
| `text_complete` | 表示本文が完全か |
| `message_origin` | `human` / `framework` / `compaction` / `unknown`。**role が user でも human とは限らない** |

### 5. 追記だけを読む — page cursor と append cursor

```bash
# 固定 snapshot の続き（前回 metadata の page_cursor）
agent-jsonl-compact view -i /path/rollout.jsonl --cursor 'c1:...' --output jsonl

# 走査完了後の追記だけ（前回 metadata の append_cursor）
agent-jsonl-compact view -i /path/rollout.jsonl --after-cursor 'c1:...' --output jsonl
```

| | page cursor | append cursor |
|---|---|---|
| 用途 | 同じ snapshot の続き | 前回の完全行末より後の追記 |
| 発行 | 未表示の primary が残っている | 全行を走査し未検査 gap が無い |
| 注意 | 途中で append cursor は出ない | 未検査 record を飛ばして前進しない |

- cursor は発行時の filter / query / scope に加えて **search の `--context` も保持**し、
  再開時に再利用します（省略時は cursor の値、明示して異なる値なら `filter_mismatch`）。
  **異なる filter を明示すると `cursor_invalid`（rc1, reason `filter_mismatch`）**、
  同じ値の明示は競合ではありません。
- `--limit` / `--tail` / `--max-events` / `--max-output-bytes` は表示条件なので、
  cursor の同一性判定には含まれません。tail の後でも同じ filter で追記を読めます。
- 置換（`generation_mismatch`）、切詰め（`truncated`）、境界書換え（`anchor_mismatch`）は
  型付きエラー（`cursor_invalid`, rc1）で停止します。暗黙の先頭再読はしません。
- 読み取り中に観測可能な変更があった場合は `status: partial` かつ `source_changed: true`
  の rc1 として返し、append cursor を発行しません（無言の完了にしない）。
- cursor は append-only 前提です。可視範囲外の prefix 全体の不変性までは保証しません。
- cursor モードは Unix の file generation（`dev`+`inode`）を使います。
  非 Unix では `cursor_unsupported_platform` で明示拒否します。

### 6. 終了コードと出力の分離

| rc | 意味 |
|---|---|
| `0` | 対象範囲を正常処理（正当な 0 件を含む） |
| `1` | source / decode / 形式未確定 / partial / cursor 不正 / 実行中変更の検出 |
| `2` | 引数・query 構文不正（`--regex` のコンパイル失敗、極小予算、旧フラグの誤用など） |

- JSONL 出力は「event 行 + 最後に metadata 行1つ」。診断・進捗は **stderr** へ出し、
  stdout に散文を混ぜません。
- 0 件は「全対象を走査して 0 件」のときだけ確定します。未検査範囲が残る場合は `partial` です。
- text 出力は ESC / OSC / C1 などの制御文字をエスケープします。tool 出力内の command は
  決して実行しません。

### 7. 予算と上限

- `--max-output-bytes`（既定 32768、最小 8192）は **stdout + stderr の合計**です。
  metadata・title・診断も含みます。収まらない場合は `budget_too_small`（rc2）で拒否し、
  途中までの不正 JSON は出しません。
- `--max-record-bytes`（既定 1 MiB）を超える行は `uninspected` として raw 範囲だけを返し、
  `status: partial` / rc1、`append_cursor` は出しません。metadata の
  `counts.uninspected.samples[].retry_cursor` を使って、より大きい上限で同じ位置から再読できます。
- 入力 record 上限と表示上限は別です。表示 cut は `text_complete: false` であって partial ではありません。

### 8. 読取 byte の読み方

metadata の `scan` には次が出ます。

| field | 意味 |
|---|---|
| `scan.read_bytes` | 主走査で論理的に消費した source byte |
| `scan.total_read_bytes` | 主走査 + 補助走査（format 判定・metadata 探索・anchor 検証・pair）の合計 |
| `scan.read_bytes_breakdown.main` / `.detect` / `.metadata` / `.anchor` / `.validation` / `.pair` | 上記の内訳 |
| `scan.with_pair.passes` / `scan.with_pair.read_bytes` | pair 追加走査のパス数と読取量 |

これらは **論理消費 byte** で、OS の物理 I/O 量でも速度でもありません。
補助走査があるため `total_read_bytes` はファイルサイズを超えることがあります。
`-i` と cursor の組み合わせでは provider / session を cursor から再利用し、旧 snapshot の
anchor 近傍と追記分だけを読みます。ただし `--with-pair` を指定した場合は、対応付けのために
同じ snapshot 内を最大2回追加で走査するため、この限りではありません（計測表の pair read を参照）。
一方 `--session` は cursor を使う前にも catalog 探索を行うため、呼び出しごとにその探索コストを
負担することがあり、追記分だけの O( suffix ) を保証するものではありません。

### 9. 限定計測（合成入力）

seed 42 で生成した合成入力（`claude_code`、110,088 records、69,234,347 B、
input sha256 `0c64ec59…a74f39a`）を、**context / regex プロファイル修正前の計測候補**
release バイナリ（sha256 `bc52b88d…4ed3ea`、linux）で測った例です。最終凍結 source の
**検証済み候補バイナリ**は sha256 `7ee5e42b…`（source 側は runner `61f297b5…`）で、独立した
テストで検証済みですが、この表の計測対象ではありません。
**一般的な性能の証明でも、両者が同じ値であるという主張でもありません**。

receipt には seed・生成パラメータ・record 件数・input の sha256・`binary_sha256`・
debug/release・`rustc` 版・platform・cache 条件を残します。

| ケース | 主走査 read | total read | wall | peak RSS | 出力 |
|---|---|---|---|---|---|
| view 先頭50件 | 23,603 B | 174,590 B | 0.0041 s | 4,652 KiB | 29,209 B |
| search 全体・末尾ヒット1件 | 69,234,347 B | 69,385,462 B | 0.6577 s | 4,736 KiB | 3,076 B |
| view --scope tools --with-pair | 179,464 B | 138,799,145 B | 2.1657 s | 4,736 KiB | 30,848 B |
| append（3行追記） | 579 B | 1,413 B | 0.0027 s | 4,268 KiB | 3,853 B |

いずれも**論理消費 byte**（OS 物理 I/O ではない）。cold/warm は未制御のため不明として扱います。
`--with-pair` の total read 増は設計上の最大2パスによります。母集団は合成1種で、
1 GB 級や実会話への一般化はしません。

再現:

```bash
cargo build --release
python3 scripts/benchmark_viewer.py generate --out temp/viewer_bench/input.jsonl
python3 scripts/benchmark_viewer.py measure \
  --input temp/viewer_bench/input.jsonl --out-dir temp/viewer_bench/out
```

### 10. 旧来の抽出はそのまま使える

閲覧系の追加は legacy 抽出を置き換えません。従来のコマンドは schema・既定値・終了コードを
維持しています。

```bash
agent-jsonl-compact -i /path/rollout.jsonl -o out --format-out both
```

`view -i FILE --format-out md` のように、**閲覧系へ旧フラグを渡すと rc2** になります
（入力形式は `--format`、出力表現は `--output text|jsonl` で指定します）。

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
just demo            # build → 3形式の同梱 fixture で利用例を実行
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
- OpenCode: 完了済み `tool_use` を `tool_call` + `tool_output` に分け、`step_finish` の
  `tokens` / `cost` / `reason` を `turn_end` に保持する。
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
src/cli.rs        CLI option、抽出、query 系(subcommand)の引数定義
src/detect.rs     Codex / Claude Code / OpenCode 自動判定(legacy 抽出用)
src/classify.rs   生 JSON レコードから正規化イベントへの展開
src/clean.rs      keep set、truncate、dedup、elide
src/render.rs     Markdown レンダリング
src/counter.rs    生レコード型・正規化 kind の集計
src/runner.rs     抽出の実行制御・集計・summary・ファイル出力・install-skills 実装
src/util.rs       JSONL streaming と JSON helper

src/query/reader.rs     bounded 物理行 reader(snapshot 境界・record 上限・pending/invalid 区別)
src/query/cursor.rs     page / append cursor のエンコード・検証
src/query/detect.rs     閲覧系の provider 判定(fail closed)
src/query/normalize.rs  provider 差の正規化(origin / private / mirror 識別)
src/query/select.rs     hard scope と soft filter
src/query/filter.rs     role / phase / 時刻 filter
src/query/toolpair.rs   call_id 対応付け(pending / orphan / budget 状態)
src/query/source.rs     session 探索・source 同一性・metadata 抽出
src/query/sessions.rs   `sessions` の候補列挙
src/query/output.rs     text / JSONL の直列化
src/query/runner.rs     閲覧系の実行制御・予算・metadata
src/query/version.rs    版と固定 source release date の単一情報源

docs/adr/                        設計判断の記録(0001 閲覧CLI / 0002 cursor と資源 / 0003 版)
docs/requirements/               要件と受入ケース
skills/agent-jsonl-compact-reader/SKILL.md  reader スキル(ビルド時にバイナリへ埋め込み)
tests/fixtures/*.jsonl                      3形式の合成fixture(実セッション由来なし)
tests/viewer_e2e.rs                         A01–A12 の受入 E2E
tests/viewer_integrity_e2e.rs               並行読取りの整合性 E2E(Linux)
tests/viewer_support/mod.rs                 隔離 HOME/XDG と fixture 生成
scripts/benchmark_viewer.py                 合成入力の生成と限定計測
.github/workflows/ci.yml       fmt-check + clippy + test
.github/workflows/release.yml  v* タグで musl tarball + sha256 を Releases へ
install.sh                     prebuilt インストーラ(sha256 検証)
```

> SKILL.md はバイナリに `include_str!` で埋め込まれます。スキル本文を変更したら再ビルドが必要です。
