# CLIリファレンス

日本語 | [English](CLI.en.md)

## 抽出のオプション

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

`sessions` / `view` / `search` は、結果をファイルに保存せず標準出力（stdout）に表示します。
入力ファイルは変更せず、既定ではキャッシュや索引も作りません。設計判断は
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
`query_schema_version`、`cursor_version` を返します。更新日はソースコードに定めた固定日です。
ビルド時刻やGitHub Releaseの更新時刻とは異なります（[ADR 0003](adr/0003-version-and-release-metadata.md)）。

### 1. 候補を探す — `sessions`

```bash
# cwd 完全一致（既定）。子ディレクトリも含めるなら --cwd-children
agent-jsonl-compact sessions --provider codex --cwd /path/to/repo --limit 20

# Claude Code も含める / 探索 root を明示する
agent-jsonl-compact sessions --provider all --root ~/.claude/projects --output jsonl
```

- 探索するのは、各ツールの既定ディレクトリと、`--root` で指定したディレクトリだけです。
  OpenCodeのrun JSONLを探す場合は `--root` を指定してください。
  既定の探索先には含まれず、`opencode.db` の場所も推測しません。
- `--provider all` はcodex / claude_code / opencodeを対象にします。
- `--scan-limit`（既定10000）に達した場合は、未走査の範囲が残っていることを `status: partial` で示します。
- 開始日時（`started_at`）と最終イベントの時刻（`last_event_at`）は別々に返します。
  ファイルの更新時刻（mtime）でセッションの実行中・停止中を判定することはありません。

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

- 既定の表示範囲（scope）は `messages` です。`--scope tools` でツールの呼出しと結果を選べます。
  `--scope all` を指定しても、private reasoning・system・developer・compactionは表示しません。
- 既定の表示件数は50件です。`--max-events` の既定値は200件で、選択したイベント（primary）、
  前後のイベント（context）、対応するツール呼出し・結果（pair）の合計に適用します。
- `--with-pair` は、選択したツール呼出しと `call_id` が一致する結果を併記します。
  対応の状態は次の5種類です。

  | 状態 | 意味 |
  |---|---|
  | `paired` | 呼出しと結果が対応している |
  | `pending_result` | 走査を完了し、結果がないことを確認した |
  | `unknown_pair` | 走査が不完全で、結果がないとは断定できない |
  | `orphan` | 対応する呼出しがない結果 |
  | `not_rendered_budget` | 対応するイベントは存在するが、出力上限のため表示していない |

- `paired_total` は対応するイベントの総数、`paired_event_ids` は保持したIDの一覧です。
  IDの保持を打ち切った場合は `paired_ids_truncated: true`、読み直す位置は `pair_reentry` で示します。
  ID配列の長さが総数と一致するとは限りません。`scan.with_pair.incomplete` は、
  対応するイベントを探す走査が不完全だったことを示します。
- `text_complete: false` は、表示本文を省略したことを示します。走査範囲が不完全という意味ではありません。

### 3. 本文を検索する — `search`

```bash
agent-jsonl-compact search -i /path/rollout.jsonl \
  --query contour_v2 --query "エラー" --context 2 --limit 10

# 正規表現は明示したときだけ
agent-jsonl-compact search -i /path/rollout.jsonl --query 'timeout|retry' --regex
```

- `--query` を複数指定すると、いずれかに一致するイベントを返します（OR）。各ヒットの `matched_queries` で、一致したクエリを確認できます。
- `matched_ranges` は、元の本文に対するUTF-8バイト位置です。表示本文を省略していても、位置は原文を基準にします。
- `--event EVENT_ID` は対象イベントを絞るだけで、出力上限は変わりません。
  全文が必要なら `--max-output-bytes`（必要なら `--max-record-bytes`）を上げて再実行するか、
  `source.byte_start` / `byte_end` で元ファイルの該当範囲を直接読んでください。
- `--context K` は、許可された表示範囲（hard scope）内で、前後K件のイベントを追加します。
  roleや時刻の絞り込み条件（soft filter）に合わない隣接イベントも `is_context: true` として表示しますが、
  reasoning / developerは含めません。
- 検索に一致したイベントが別のヒットの前後にも含まれる場合は、primaryとして1回だけ表示します。

### 4. 元ログの位置を確認する

JSONLのイベント行には、元ログを読み直すための位置情報などが含まれます。

| フィールド | 意味 |
|---|---|
| `event_id` | 入力ファイルの識別情報、元の開始位置、行内のイベント位置から作る安定したID |
| `source.raw_line` | 元ログの行番号（1始まり） |
| `source.byte_start` / `byte_end` | 元ログのバイト範囲（0始まり、終了位置のバイトは含まない） |
| `source.subevent_index` | 元ログの1行から複数イベントを取り出した場合の行内位置 |
| `text_complete` | 表示本文が完全か |
| `message_origin` | `human` / `framework` / `compaction` / `unknown`。roleがuserでもhumanとは限らない |

### 5. 続きや追記を読む（カーソル）

カーソルは、前回の読み取り位置と条件を記録した値です。既存範囲の続きを読むpage cursorと、
走査完了後の追記を読むappend cursorを使い分けます。snapshotは、読み取り対象として固定したファイル範囲を指します。

```bash
# 固定 snapshot の続き（前回 metadata の page_cursor）
agent-jsonl-compact view -i /path/rollout.jsonl --cursor 'c1:...' --output jsonl

# 走査完了後の追記だけ（前回 metadata の append_cursor）
agent-jsonl-compact view -i /path/rollout.jsonl --after-cursor 'c1:...' --output jsonl
```

| | page cursor | append cursor |
|---|---|---|
| 用途 | 同じsnapshotの続き | 前回の完全行末より後の追記 |
| 発行 | 未表示のprimaryが残っている | 全行を走査し、未検査の行がない |
| 注意 | ページの途中ではappend cursorは出ない | 未検査の行を飛ばして先へ進まない |

- カーソルは発行時のフィルター、クエリ、表示範囲と、searchの `--context` を保持します。
  再開時に条件を省略すると、カーソル内の値を使います。同じ値の指定も可能です。
  異なる値を指定すると `cursor_invalid`（終了コード1、reason `filter_mismatch`）になります。
- `--limit` / `--tail` / `--max-events` / `--max-output-bytes` は表示条件なので、
  カーソルとの一致を検証する条件には含まれません。tailの後でも同じフィルターで追記を読めます。
- 入力ファイルの置換（`generation_mismatch`）、切り詰め（`truncated`）、検証位置の書き換え（`anchor_mismatch`）を
  検出すると、`cursor_invalid`（終了コード1）で停止します。自動で先頭から読み直すことはありません。
- 読み取り中に変更を検出すると、`status: partial` と `source_changed: true` を返して終了コード1で停止します。
  append cursorは発行しません。
- カーソルは、ファイルが追記だけで更新されることを前提とします。検証範囲外の読み取り済み部分まで、変更がないとは保証しません。
- カーソルを使う読み取りでは、Unixの `dev` と `inode` でファイルを識別します。
  Unix以外では `cursor_unsupported_platform` エラーになります。

### 6. 終了コードと出力の分離

| 終了コード | 意味 |
|---|---|
| `0` | 対象範囲を正常処理（正当な0件を含む） |
| `1` | 入力の読み取り・デコードの失敗、形式を判定できない、走査が不完全、不正なカーソル、読み取り中の変更を検出 |
| `2` | 引数・クエリ構文の誤り（`--regex` のコンパイル失敗、小さすぎる出力上限、旧フラグの誤用など） |

- JSONLでは、イベント行の後にメタデータ行を1行出力します。診断や進捗は標準エラー出力（stderr）に出し、
  stdoutには説明文を混ぜません。
- 0件は「全対象を走査して0件」のときだけ確定します。未検査範囲が残る場合は `partial` です。
- テキスト出力ではESC / OSC / C1などの端末制御文字をエスケープします。ツール出力内のコマンドは実行しません。

### 7. 入力行と出力の上限

- `--max-output-bytes`（既定32768、最小8192）はstdout + stderrの合計です。
  メタデータ、タイトル、診断も含みます。収まらない場合は `budget_too_small`（終了コード2）で停止し、
  途中で切れた不正なJSONは出力しません。
- `--max-record-bytes`（既定1 MiB）を超える行は、`uninspected` として元ログの範囲だけを返します。
  `status: partial`、終了コード1となり、`append_cursor` は発行しません。メタデータの
  `counts.uninspected.samples[].retry_cursor` を使って、より大きい上限で同じ位置から再読できます。
- 入力行サイズと表示量の上限は別々です。表示本文を省略しただけなら `text_complete: false` となり、`partial` にはなりません。

### 8. 読み取ったバイト数

メタデータの `scan` で、読み取り量を確認できます。

| フィールド | 意味 |
|---|---|
| `scan.read_bytes` | 主走査で読み取った入力のバイト数 |
| `scan.total_read_bytes` | 主走査と補助走査（形式判定・メタデータ探索・カーソル検証位置の確認・ツールの対応付け）の合計 |
| `scan.read_bytes_breakdown.main` / `.detect` / `.metadata` / `.anchor` / `.validation` / `.pair` | 上記の内訳 |
| `scan.with_pair.passes` / `scan.with_pair.read_bytes` | ツールの対応付けのために追加で走査した回数とバイト数 |

これらはCLIが論理的に読み取ったバイト数です。OSの物理I/O量や処理速度を表す値ではありません。
補助走査があるため、`total_read_bytes` はファイルサイズを超えることがあります。

`-i` とカーソルを組み合わせると、ログ形式とセッションの情報をカーソルから再利用します。
読むのは、前回のsnapshotの検証位置（anchor）付近と追記分だけです。
ただし `--with-pair` を指定すると、対応するイベントを探すために同じsnapshotを最大2回追加で走査します。

`--session` は、カーソルを使う前にセッション一覧を探索します。呼出しごとにその処理が必要になる場合があるため、
追記分だけに比例する処理量（O(suffix)）は保証しません。

抽出と閲覧のフラグは別です。`view --format-out md` は引数エラーになります。
