# CLIリファレンス

日本語 | [English](CLI.en.md)

## Options (抽出)

```text
--format auto|codex|claude_code|opencode
--channel terminal|api|both      # Codex のみ有効
--msg-chars N                    # 0 は全文保持
--out-chars N                    # 0 は全文保持
--elide-outputs                  # 肥大ツール出力を件数と先頭行へ畳む
--keep-token-count               # Codex token_count を保持
--no-dedup                       # 重複畳み込みを無効化
--format-out jsonl|md|both       # 既定 jsonl(clean のみ)
--summary                        # summary JSON も出力
--stats                          # 形式とレコード型分布のみ表示して終了
--name <stem>                    # 出力ベース名(既定は入力 stem)
```

## 閲覧CLI

既存の抽出が「ファイルへ保存する」のに対し、`sessions` / `view` / `search` は
保存せずstdoutへ返す閲覧用のコマンドです。sourceは変更せず、cacheやindexも
既定では作りません。詳細な設計判断は
[ADR 0001](adr/0001-additive-session-viewer-cli.md) /
[ADR 0002](adr/0002-cursor-and-resource-boundaries.md) にあります。

基本操作

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
`query_schema_version`、`cursor_version` を返します。更新日はsourceの固定日で、
build時刻でもGitHub Releaseの更新時刻でもありません（[ADR 0003](adr/0003-version-and-release-metadata.md)）。

### 1. 候補を探す — `sessions`

```bash
# cwd 完全一致（既定）。子ディレクトリも含めるなら --cwd-children
agent-jsonl-compact sessions --provider codex --cwd /path/to/repo --limit 20

# Claude Code も含める / 探索 root を明示する
agent-jsonl-compact sessions --provider all --root ~/.claude/projects --output jsonl
```

- 探索対象はproviderごとの既知rootと、`--root` で明示したrootのみです。
  OpenCodeのrun JSONLは既定rootでは探しません（`opencode.db` を推測しないため）。
  使うときは `--root` で明示してください。
- `--provider all` はcodex / claude_code / opencodeを対象にします。
- `--scan-limit`（既定10000）に達した場合は `status: partial` として未走査を明示します。
- 表示は開始日時 (`started_at`) と最終event時刻 (`last_event_at`) を別々に返します。
  mtimeは実行中・停止中の判定には使いません。

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

- 既定scopeは `messages` です。`--scope tools` でtool、`--scope all` でも
  private reasoning・system・developer・compactionは出ません。
- 既定の表示件数は50件、`--max-events`（primary + context + pairの総量）は200件です。
- `--with-pair` を付けると、選択したtool呼出しに対応する結果を `call_id` で併記します。
  状態は `paired` / `pending_result`（走査完了後に不在を確認）/ `unknown_pair`（走査が
  不完全で未着と断定できない）/ `orphan`（呼出し無しの結果）/ `not_rendered_budget`
  （存在するが予算で未表示）の5つです。
- 対応件数は `paired_total`（実総数）と `paired_event_ids`（保持したID）を分けて返し、
  打ち切り時は `paired_ids_truncated: true`、再入位置は `pair_reentry` で示します。
  配列の長さを相手の総数と読み替えないでください。`scan.with_pair.incomplete` は
  pair走査自体が不完全だったことを示します。
- `text_complete: false` は「表示がcutされた」意味で、取得範囲が不完全という意味ではありません。

### 3. 本文を検索する — `search`

```bash
agent-jsonl-compact search -i /path/rollout.jsonl \
  --query contour_v2 --query "エラー" --context 2 --limit 10

# 正規表現は明示したときだけ
agent-jsonl-compact search -i /path/rollout.jsonl --query 'timeout|retry' --regex
```

- `--query` の繰返しはOR。各ヒットは `matched_queries` を持ちます。
- `matched_ranges` は 原文textに対するUTF-8 byte offsetです。表示がcutされていても
  位置は原文基準です。
- `--event EVENT_ID` は対象eventを絞るだけで、同じ予算のまま全文が返る保証はありません。
  全文が必要なら `--max-output-bytes`（必要なら `--max-record-bytes`）を上げて再実行するか、
  `source.byte_start` / `byte_end` でrawの該当範囲を直接読んでください。
- `--context K` はhard scope内の前後K event。roleや時刻のsoft filterを越えた隣接eventは
  `is_context: true` として明示されますが、reasoning / developerは決して含まれません。
- 同じeventがヒットかつ他ヒットの文脈である場合は、primaryとして1回だけ出ます。

### 4. 根拠と元位置

event行は次を持ちます。

| field | 意味 |
|---|---|
| `event_id` | source同一性 + raw開始位置 + subeventから作る安定ID |
| `source.raw_line` | raw行番号（1始まり） |
| `source.byte_start` / `byte_end` | raw byte範囲（0始まり・終端排他） |
| `source.subevent_index` | 1 rawが複数eventになる場合のindex |
| `text_complete` | 表示本文が完全か |
| `message_origin` | `human` / `framework` / `compaction` / `unknown`。roleがuserでもhumanとは限らない |

### 5. 追記だけを読む — page cursorとappend cursor

```bash
# 固定 snapshot の続き（前回 metadata の page_cursor）
agent-jsonl-compact view -i /path/rollout.jsonl --cursor 'c1:...' --output jsonl

# 走査完了後の追記だけ（前回 metadata の append_cursor）
agent-jsonl-compact view -i /path/rollout.jsonl --after-cursor 'c1:...' --output jsonl
```

| | page cursor | append cursor |
|---|---|---|
| 用途 | 同じsnapshotの続き | 前回の完全行末より後の追記 |
| 発行 | 未表示のprimaryが残っている | 全行を走査し未検査gapが無い |
| 注意 | 途中でappend cursorは出ない | 未検査recordを飛ばして前進しない |

- cursorは発行時のfilter / query / scopeに加えてsearchの `--context` も保持し、
  再開時に再利用します（省略時はcursorの値、明示して異なる値なら `filter_mismatch`）。
  異なるfilterを明示すると `cursor_invalid`（rc1, reason `filter_mismatch`）、
  同じ値の明示は競合ではありません。
- `--limit` / `--tail` / `--max-events` / `--max-output-bytes` は表示条件なので、
  cursorの同一性判定には含まれません。tailの後でも同じfilterで追記を読めます。
- 置換（`generation_mismatch`）、切詰め（`truncated`）、境界書換え（`anchor_mismatch`）は
  型付きエラー（`cursor_invalid`, rc1）で停止します。暗黙の先頭再読はしません。
- 読み取り中に観測可能な変更があった場合は `status: partial` かつ `source_changed: true`
  のrc1として返し、append cursorを発行しません（無言の完了にしない）。
- cursorはappend-only前提です。可視範囲外のprefix全体の不変性までは保証しません。
- cursorモードはUnixのfile generation（`dev`+`inode`）を使います。
  非Unixでは `cursor_unsupported_platform` で明示拒否します。

### 6. 終了コードと出力の分離

| rc | 意味 |
|---|---|
| `0` | 対象範囲を正常処理（正当な0件を含む） |
| `1` | source / decode / 形式未確定 / partial / cursor不正 / 実行中変更の検出 |
| `2` | 引数・query構文不正（`--regex` のコンパイル失敗、極小予算、旧フラグの誤用など） |

- JSONL出力は「event行 + 最後にmetadata行1つ」。診断・進捗はstderrへ出し、
  stdoutに散文を混ぜません。
- 0件は「全対象を走査して0件」のときだけ確定します。未検査範囲が残る場合は `partial` です。
- text出力はESC / OSC / C1などの制御文字をエスケープします。tool出力内のcommandは
  決して実行しません。

### 7. 予算と上限

- `--max-output-bytes`（既定32768、最小8192）はstdout + stderrの合計です。
  metadata・title・診断も含みます。収まらない場合は `budget_too_small`（rc2）で拒否し、
  途中までの不正JSONは出しません。
- `--max-record-bytes`（既定1 MiB）を超える行は `uninspected` としてraw範囲だけを返し、
  `status: partial` / rc1、`append_cursor` は出しません。metadataの
  `counts.uninspected.samples[].retry_cursor` を使って、より大きい上限で同じ位置から再読できます。
- 入力record上限と表示上限は別です。表示cutは `text_complete: false` であってpartialではありません。

### 8. 読取byteの読み方

metadataの `scan` には次が出ます。

| field | 意味 |
|---|---|
| `scan.read_bytes` | 主走査で論理的に消費したsource byte |
| `scan.total_read_bytes` | 主走査 + 補助走査（format判定・metadata探索・anchor検証・pair）の合計 |
| `scan.read_bytes_breakdown.main` / `.detect` / `.metadata` / `.anchor` / `.validation` / `.pair` | 上記の内訳 |
| `scan.with_pair.passes` / `scan.with_pair.read_bytes` | pair追加走査のパス数と読取量 |

これらは 論理消費byteで、OSの物理I/O量でも速度でもありません。
補助走査があるため `total_read_bytes` はファイルサイズを超えることがあります。
`-i` とcursorの組み合わせではprovider / sessionをcursorから再利用し、旧snapshotの
anchor近傍と追記分だけを読みます。ただし `--with-pair` を指定した場合は、対応付けのために
同じsnapshot内を最大2回追加で走査するため、この限りではありません。
一方 `--session` はcursorを使う前にもcatalog探索を行うため、呼び出しごとにその探索コストを
負担することがあり、追記分だけのO( suffix ) を保証するものではありません。


抽出と閲覧のフラグは別です。`view --format-out md` は引数エラーになります。
