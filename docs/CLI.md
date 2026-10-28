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

`version --output json` は `name`、`version`、固定の `source_release_date`、`source_commit`、
`source_commit_dirty`、`query_schema_version`、`cursor_version`、`source_date_kind` を返します。
`source_release_date` はソースコードに定めた固定のリリース日です。
`source_date_kind` は、この日付がビルド時刻ではないことを示します。
`source_commit` はビルド時に取得した Git コミット（40桁または64桁の16進）で、取得できないビルドでは `unknown` です。
`source_commit_dirty` はビルド時に未コミット変更があったかを示し、Git を利用できないビルドでは `null`（不明）です。
不明は「未コミットの変更なし」とは扱いません。増分ビルドでもブランチの前進や作業ツリーの変更を反映するため、ビルドスクリプトは毎回 Git 状態を確認します。
ビルド時刻やGitHub Releaseの更新時刻とは異なります（[ADR 0003](adr/0003-version-and-release-metadata.md)）。

### 1. 候補を探す — `sessions`

```bash
# cwd 完全一致（既定）。子ディレクトリも含めるなら --cwd-children
agent-jsonl-compact sessions --provider codex --cwd /path/to/repo --limit 20

# Claude Code も含める / 探索 root を明示する
agent-jsonl-compact sessions --provider all --root ~/.claude/projects --output jsonl
```

- 各ツールの既定ディレクトリを探索します。`--root` を指定した場合は、そのディレクトリを探索します。
  OpenCodeのrun JSONLは既定の探索先に含まれないため、`--root` を指定してください。
  `opencode.db` の場所も推測しません。
- `--provider all` はcodex / claude_code / opencodeを対象にします。
- 既定 root が存在しない場合は意図的に読み飛ばします。`--root` で明示した root が存在しない、
  または権限などで読めない場合は `status: partial`（`scan_io`）です。
- root 内のシンボリックリンクは辿りません。スキップした symlink は `symlinks_skipped` に数え、
  `status: partial`（`scan_symlink`）として未確認範囲を残します。root 自体がディレクトリへの
  symlink の場合は辿ります。
- 1ファイルが64 MiBを超える場合は `scan_file_bytes`、探索全体が256 MiBを超える場合は
  `scan_bytes` で `status: partial` になります。
- `--scan-limit`（既定10000）に達した場合は、未走査の範囲が残っていることを `status: partial` で示します。
- `--session` は要求した session id だけを照合するため、無関係な重複 id があっても一意な
  対象は解決できます。`sessions` 一覧は完全走査で重複 id があれば `session_ambiguous` です。
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
- `view` の既定表示件数は50件、`search` の既定ヒット数は20件です。
  `--max-events` の既定値は200件（上限10,000）で、選択したイベント（primary）、
  前後のイベント（context）、対応するツール呼出し・結果（pair）の合計に適用します。
- `--with-pair` は、選択したツール呼出しと `call_id` が一致する結果を併記します。
  対応の状態は次の6種類です。

  | 状態 | 意味 |
  |---|---|
  | `paired` | 呼出しと結果が対応している |
  | `pending_result` | 走査を完了し、結果がないことを確認した |
  | `unknown_pair` | 走査が不完全で、結果がないとは断定できない |
  | `orphan` | 対応する呼出しがない結果 |
  | `not_rendered_budget` | 対応するイベントは存在するが、出力上限のため表示していない |
  | `ambiguous` | 同じ call_id が同じ側に複数あり、一意に対応付けられない。`pair_reason` で理由を示す |

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
  reasoning / developerは含めません。K の上限は10,000です。保持上限を超えた文脈は
  `counts.context_dropped` に件数を出して省略します。
- `--phase` には、ログに記録された phase 名をカンマ区切りで指定します。
  `--until` は指定時刻より前のイベントに絞り込み、境界の時刻は含みません。
  `--since` / `--until` は offset 付き RFC3339 を要求します。
- `search --with-pair` も、ヒットした tool 呼出し・結果に対応する相手を併記します。
- 検索に一致したイベントが別のヒットの前後にも含まれる場合は、primaryとして1回だけ表示します。
- 1イベントの一致位置は最大256件まで保持し、超過時は `matched_ranges_truncated: true` と
  `matched_ranges_total` を返します。`matched_ranges_total` は観測した一致数で、省略がなければ
  正確な総数、省略時は下限です。正規表現はクエリごとに 257 件（256件を超える最小の観測）で
  列挙を打ち切ります。`matched_queries` は省略しません。

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
| 発行条件 | 未表示のprimaryが残っているとき | 全行を走査し、未検査の行がないとき |
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

- `--max-output-bytes`（既定32768、最小8192）は標準出力と標準エラー出力の合計です。
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

### 9. 出力フィールドと失敗の種類

#### イベント行

| フィールド | 意味 |
|---|---|
| `event_id` | `source_id:byte_start:subevent_index`。行番号は含まない |
| `source.raw_line` | 表示用の行番号。cursor 再開時は継承値 |
| `source.raw_line_verified` | 行番号をこの走査で確認したか。cursor 継承時は `false` |
| `timestamp` / `epoch_ms` | 原文の時刻と、解析できた場合の epoch ミリ秒 |
| `matched_ranges` | 原文 text に対する UTF-8 byte 範囲。最大256件 |
| `matched_ranges_total` / `matched_ranges_truncated` | 観測した一致数（省略時は下限。正規表現はクエリごとに257件で打切り）と、256件で省略したか |
| `pair_state` / `pair_reason` / `paired_event_ids` | tool 対応の状態・理由・保持した相手 ID |
| `text_complete` | 表示本文を省略したか。走査の不完全とは別 |

#### メタデータ行

`status` は `ok` / `partial` / `error` のいずれかです。検索の走査が30秒を超えた場合は `scan.search_timed_out: true` と `partial` になります。
`partial` は、scan limit、未検査 record、
不正 record、読取不能ファイル、読み取り中の変更など、確定できない範囲が残ることを示します。
`counts.invalid` / `counts.uninspected` / `counts.context_dropped` と `scan` の各値を確認してください。

`sessions` の `scan` には `files_scanned`、`scan_limit`、`scan_limit_reached`、`entries_visited`、
`scan_bytes`、`walk_errors`、`symlinks_skipped`、`limit_reason` が入ります。

`limit_reason` は
`scan_limit` / `scan_entries` / `scan_depth` / `scan_bytes` / `scan_file_bytes` / `scan_io` /
`scan_symlink` / `scan_time` のいずれかです。存在しない既定 root は意図的に読み飛ばしますが、
明示 root の不在や権限不足などで読めない root・ディレクトリ・エントリは `walk_errors` に数え、
`status: partial` として扱います。辿らずにスキップした symlink は `symlinks_skipped` に数え、
`scan_symlink` として同じく partial にします。

#### 主なエラー

| type | 意味 |
|---|---|
| `session_not_found` | 宣言した root と完全な探索で候補がない |
| `session_ambiguous` | 完全な探索で同じ session id が複数ある |
| `session_resolution_incomplete` | scan limit・読取不能・未検査・不正 record が残り、一意性や不在を断定できない |
| `cursor_invalid` | cursor の形式・世代・位置・filter・session が一致しない |
| `budget_too_small` | 出力予算に収まらず、進捗のあるページを返せない（終了コード2） |
| `cursor_too_large` | `--query` などの指定により64 KiBを超えるカーソルができるため、成功として返せない（終了コード2） |
| `source_unreadable` | 入力が読めない、または通常ファイルでない |

入力の行数・深さ・バイト数には探索側の上限があります。上限に達した結果は成功ではなく
`partial`（終了コード1）として返し、未探索範囲を成功扱いしません。
