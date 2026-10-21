---
name: agent-jsonl-compact-reader
description: >-
  Compact and read large Codex CLI, Claude Code, or OpenCode run JSONL logs
  without loading the raw file into context. Use when asked to read, summarize,
  inspect, or investigate an existing session transcript, especially when the
  raw JSONL is large.
  Triggers: 過去セッションのjsonlを読む/要約する, セッションログを軽量化して読み込む,
  rollout jsonl を読む, transcript jsonl を要約, agent-jsonl-compact で抽出.
---

# agent-jsonl-compact-reader

巨大な Codex / Claude Code / OpenCode run セッション JSONL を、生のままコンテキストへ
載せずに読むためのスキル。0.3.0 以降は **閲覧系コマンド**で「探す → 必要箇所を読む →
根拠の raw 位置へ戻る」を行い、必要なら従来の**忠実 export** を併用する。

## When to use

- 「この過去セッションの jsonl を読んで / 要約して」
- `~/.codex/sessions/YYYY/MM/DD/rollout-*.jsonl` や
  `~/.claude/projects/<proj>/<uuid>.jsonl` の内容把握・調査
- `opencode run --format json` を保存した NDJSON の内容把握・調査
- 生 JSONL が大きく、全文を読むとコンテキストを圧迫する場合
- 「どのコマンドを実行したか」「ある処理の根拠」を tool 呼出しから辿りたい場合

入力の典型的な所在:

```text
Codex       ~/.codex/sessions/<YYYY>/<MM>/<DD>/rollout-*.jsonl
Claude Code ~/.claude/projects/<project-slug>/<uuid>.jsonl
OpenCode    保存先は任意(opencode run --format json > opencode-session.jsonl)
```

OpenCode は通常 `opencode.db` に永続化し、JSONL を自動保存しない。`opencode export` の
単一 JSON 文書も対象外。対応入力が必要なら stdout を明示的に保存する。
OpenCode run JSONL にはユーザープロンプトとモデル名が含まれないため、結果にも現れない。

## Step 0 — バイナリと版を確認する

```bash
command -v agent-jsonl-compact && agent-jsonl-compact --version
agent-jsonl-compact version --output json   # source_release_date / schema 版
```

**閲覧系（`sessions` / `view` / `search` / `version`）は 0.3.0 以降**にしかない。
`--version` が 0.2.x 以下なら:
- そのバイナリを更新する（このスキルは global インストールを勝手に書き換えない）、または
- 閲覧系を使わず、下記 Step 7 の legacy 抽出（`-i/-o`）へフォールバックする。

無ければ導入（どちらか）:

```bash
curl -fsSL https://raw.githubusercontent.com/yuki-inaho/agent-jsonl-compact/main/install.sh | bash
just install        # リポジトリ内でソースから
```

スキル自体はバイナリから配置できる:

```bash
agent-jsonl-compact install-skills                 # 両方(存在する側)
agent-jsonl-compact install-skills --claude-only   # または --codex-only
```

## Step 1 — セッション候補を探す（`sessions`）

```bash
agent-jsonl-compact sessions --provider codex --cwd /path/to/repo --limit 20
agent-jsonl-compact sessions --provider all --output jsonl
```

- `--cwd` は metadata 上の **完全一致**。子ディレクトリも含めるなら `--cwd-children`。
- 開始日 (`started_at`) と最終 event 時刻 (`last_event_at`) は別物。開始が古くても
  当日更新されている会話は見つかる。mtime は実行中判定に使わない。
- **OpenCode は既定 root では探さない**（DB パスを推測しない）。`--root` で明示する。
- `status: partial` は「候補集合が完全でない」意味。原因は `--scan-limit` 到達だけではなく、
  走査中の record が invalid / uninspected だった場合も含む。metadata の
  `scan.scan_limit_reached`・`records_invalid`・`records_uninspected` を見て理由を区別し、
  候補の網羅性を主張しない。

## Step 2 — 必要箇所だけ読む（`view`）

```bash
agent-jsonl-compact view --session codex:SESSION_ID --tail 30
agent-jsonl-compact view -i <input.jsonl> --since 2026-10-21T00:00:00+09:00 \
  --role user,assistant --output jsonl
```

- 既定は `messages` scope。tool は `--scope tools`。`--scope all` でも
  private reasoning / system / developer / compaction は出ない。
- 既定 50 件。`--max-events` は primary + context + pair の**総量**上限（既定 200）。
- `--with-pair` で tool 呼出しと結果を `call_id` で併記する。
  `pending_result`（結果未着）/ `orphan`（呼出し無しの結果）/ `not_rendered_budget`
  （存在するが予算で未表示）を区別する。**rc0 でも依頼作業が完了したとは判断しない。**
- `text_complete: false` は「表示が cut された」だけ。取得範囲の欠落ではない。

## Step 3 — 本文を検索する（`search`）

```bash
agent-jsonl-compact search -i <input.jsonl> \
  --query contour_v2 --query "エラー" --context 2 --limit 10
agent-jsonl-compact search -i <input.jsonl> --query 'timeout|retry' --regex
```

- `--query` の繰返しは **OR**。`matched_queries` でどの語に一致したか分かる。
- `matched_ranges` は **原文 text の UTF-8 byte offset**。表示が cut されていても
  位置は原文基準である。
- `--event EVENT_ID` は**その event に絞る**だけで、同じ予算のまま全文が返る保証はない。
  全文が必要なら `--max-output-bytes`（必要なら `--max-record-bytes`）を上げて再実行するか、
  `source.byte_start`/`byte_end` を使って raw ファイルの該当範囲を直接読む。
  `text_complete: false` のまま「全文を読んだ」と扱わない。
- `--context K` は hard scope 内の前後 K event。soft filter を越えた隣接は
  `is_context: true` として明示されるが、reasoning / developer は含まれない。
- 同じ event がヒットかつ他ヒットの文脈なら、primary として1回だけ出る。

## Step 4 — 根拠（raw 位置と tool 対応）を読む

JSONL 出力の event は次を持つ。**推測で本文を要約するより、この位置を辿る方が確実**。

| field | 用途 |
|---|---|
| `source.raw_line` / `byte_start` / `byte_end` | 元ファイルの該当行・byte 範囲 |
| `source.subevent_index` | 1 raw 行が複数 event になる場合の index |
| `event_id` | 再読・参照用の安定 ID（`--event` に渡せる） |
| `message_origin` | `human` / `framework` / `compaction` / `unknown`。**role=user でも human とは限らない** |
| `tool_name` / `call_id` / `paired_event_ids` / `pair_state` | tool の対応関係 |
| `paired_total` / `paired_ids_truncated` / `pair_reentry` | 対応の実総数・ID 打ち切り・再入位置 |

`pair_state` の読み分け（`--with-pair` 指定時）:

| state | 意味 | 注意 |
|---|---|---|
| `paired` | call と結果が揃っている | — |
| `pending_result` | snapshot を最後まで走査し、結果が無いことを確認した | 「未着」と断定してよいのはこの場合だけ |
| `unknown_pair` | 未検査 record や gap があり、未着と断定できない | `scan.with_pair.incomplete: true` なら証明が欠けている |
| `orphan` | 結果だけがあり対応する call が無い | — |
| `not_rendered_budget` | 対応は存在するが表示予算で出せない | 未着でも unknown でもない |

- `paired_total` は**実総数**、`paired_event_ids` は保持した ID にすぎない。
  `paired_ids_truncated: true` のとき、配列の長さを相手の総数と読み替えない。
  未保持分は `pair_reentry`（raw line / byte 開始）から辿る。
- 終端がまだ書き込み中で newline が無い PendingTail は `pending_result` のままで、
  `incomplete` を立てない（未完の末尾と証明の欠落は別）。

## Step 5 — 追記だけを読む（page / append cursor）

```bash
# 同じ snapshot の続き
agent-jsonl-compact view -i <input.jsonl> --cursor 'c1:...' --output jsonl
# 前回の完全行末より後の追記だけ
agent-jsonl-compact view -i <input.jsonl> --after-cursor 'c1:...' --output jsonl
```

- `page_cursor` は固定 snapshot の続き。`append_cursor` は**走査完了後**の追記用。
  途中ページで append cursor は出ない。
- cursor は発行時の filter / query / scope と search の `--context` を再利用する。
  `--context` を省略した再開は cursor の値を用いる。**違う値を明示すると
  `cursor_invalid`（rc1, `filter_mismatch`）**。同じ値の明示は競合ではない。
- `--limit` / `--tail` / `--max-events` / `--max-output-bytes` は表示条件で、
  cursor の同一性には含まれない。tail の後でも同じ filter で追記を読める。
- source の置換・切詰め・境界書換えは型付きエラーで止まる。暗黙の先頭再読はしない。
- cursor は Unix の file generation を使う。非 Unix では明示拒否される。

## Step 6 — 終了コードと予算の扱い

| rc | 意味 | 取るべき行動 |
|---|---|---|
| `0` | 正常処理（正当な 0 件を含む） | 結果を読む |
| `1` | partial / 形式未確定 / source 不正 / cursor 不正 | metadata の `status` と `counts` を見る。未検査範囲を「0件」と読み替えない |
| `2` | 引数・query 構文不正 | 引数を直す（極小 `--max-output-bytes` もここ） |

- `--max-output-bytes`（既定 32768、最小 8192）は stdout + stderr の合計。
- `--max-record-bytes`（既定 1 MiB）を超える行は `uninspected` になり `status: partial`。
  `counts.uninspected.samples[].retry_cursor` で、より大きい上限から同じ位置を再読できる。
- 形式が不明・混在のときは **推測に任せず `--format` を明示する**
  （`auto` は `format_uncertain` / `format_inconsistent` で fail closed する）。

## Step 7 — 忠実 export が要るとき（従来機能）

閲覧系は保存しない。全文の忠実な縮約を残したいときは従来の抽出を使う。
schema・既定値・終了コードは 0.2.0 から変わらない。

```bash
agent-jsonl-compact -i <input.jsonl> -o temp/session_extracts
agent-jsonl-compact -i <input.jsonl> --stats
agent-jsonl-compact -i <rollout.jsonl> -o temp/session_extracts_api --channel api
```

生成物は `<name>.clean.jsonl` / `<name>.transcript.md` / `<name>.summary.json`。
`--msg-chars` / `--out-chars` / `--elide-outputs` はサイズ優先のときだけ明示する。

Codex の新しい rollout で本文が空に見えるときは `--channel api`（または `both`）を使う。
code mode の `custom_tool_call` / `custom_tool_call_output` は legacy 抽出では落ちるため、
必要な場合は閲覧系の `--scope tools` で読む方が確実である。
reasoning が暗号化されている場合は復元できない。

## Notes

- 既定はシグナル保持優先。サイズ削減は lossy ノブを明示したときだけ効く。
- 成果物は会話本文・ホームパス等 **PII を含みうる。コミットしない。**
- この CLI は実行時に LLM・ネットワーク・外部エージェント基盤を要求しない。
