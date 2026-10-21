# ADR 0001 — 既存exportを保ったまま追加する閲覧CLI

- Status: **Accepted / Implemented / 本記録は局所検証（local evidence）まで。**
  CI・release の進行状況は [GitHub Actions](https://github.com/yuki-inaho/agent-jsonl-compact/actions) と
  [Releases](https://github.com/yuki-inaho/agent-jsonl-compact/releases) を参照（本書は将来の通過を主張しない）。
- Date: 2026-10-21
- 対象バージョン: 0.3.0 / query schema v1
- 関連: [ADR 0002](0002-cursor-and-resource-boundaries.md), [ADR 0003](0003-version-and-release-metadata.md), `docs/requirements/session_viewer_cli.md`

## 1. 解こうとした問題

会話ログから「今日の依頼」「直近の回答」「ある処理を実行した根拠」を読むとき、利用者は毎回
次の作業を手で組み合わせていた。

1. session ファイルを日付フォルダ・mtime・cwd から探す
2. role や日時で絞ってから、必要件数だけを表示する
3. keyword で検索し、ヒットの前後を読む
4. tool 呼出しと結果を `call_id` で突き合わせる
5. 追記分だけを、前回の既読位置から読む

現行の抽出（`clean.jsonl` / `transcript.md` / `summary.json`）は忠実な縮約としては機能するが、
上記の1〜5は毎回「書き出してから別コマンドで切り出す」必要があり、更新のたびに全量再出力が
発生していた。要件書 §2 の観測（U01–U08, L01–L02）がこの問題の根拠である。

**要件（P0）**: F01 session 発見 / F02 発言の既定表示と filter / F03 keyword・regex 検索と文脈 /
F04 tool 呼出しと結果の対応 / F05 raw 位置への復元 / F06 追記差分 cursor / F07 件数・byte 上限 /
F08 stdout 返却と診断分離 / F09 legacy 互換 / F10 形式未確定・不正・未完の区別。
非機能 N01–N05（全件非保持、差分読取り、計測分離、無書き込み、制御文字と非実行）。

## 2. 決定

**既存の抽出 CLI を一切置き換えず、`sessions` / `view` / `search` / `version` を追加する。**

- root の legacy 抽出フラグ（`-i/-o/--name/--format/--format-out/--channel/--msg-chars/
  --out-chars/--keep-token-count/--no-dedup/--elide-outputs/--stats`）と `install-skills` は
  スキーマ・既定値・終了コードを維持する（`src/cli.rs` の `Cli` と `SharedQueryArgs` は別型）。
- 新コマンドは別の終了コード契約を持つ: `0` 正常処理（正当な0件を含む）/ `1` source・decode・
  形式未確定・partial・cursor 不正 / `2` 引数・query 構文不正。
- 新規出力は query schema v1（`schema_version: 1`）として legacy clean schema と分離する
  （[src/query/model.rs](../../src/query/model.rs)）。`event` 行と最後の `metadata` 行だけで構成し、診断は stderr。

### 2.1 共通の bounded reader

[src/query/reader.rs](../../src/query/reader.rs) の `SourceReader` が物理行を1本ずつ返す。設計上の不変条件:

- open 時点の `snapshot_end = file.metadata().len()` を固定し、**それ以降の追記を読まない**
  （`reader.rs` の `fill_buf` / `consume` ループは `snapshot_end` で窓を切る）。
- `max_record_bytes`（既定 1 MiB）を超える行は本文を保持せず `Uninspected` として
  byte 範囲だけを返す。後から大きい上限で同じ位置を再読できる。
- newline で終わらない末尾は `PendingTail`（追記待ち）とし、途中の不正行（`invalid`）と区別する。
- `read_stream_bytes` は **論理的に消費した source byte**（パスごと）。OS の物理 I/O 量ではない。

### 2.2 provider 差と origin / private hard scope

- 3形式（Codex / Claude Code / OpenCode run JSONL）を `--format` 明示または内容からの証拠判定で
  選ぶ。判定不能は `format_uncertain`、認識済み provider の混在は `format_inconsistent` で
  fail closed し、推測した provider の 0 件成功に隠さない（[src/query/detect.rs](../../src/query/detect.rs)）。
- `message_origin` は source 上で判別できる範囲で `human` / `framework` / `compaction` /
  `unknown` を持つ。**role が user であることは human の根拠にしない**。
- hard scope（`messages` / `tools` / `all`）は reasoning・system・developer・compaction を
  常に除外する。soft filter（role / phase / 時刻）は context で越え得るが、hard scope は越えない
  （[src/query/select.rs](../../src/query/select.rs), [src/query/filter.rs](../../src/query/filter.rs)）。
- 本文が同じだけの別発言は削除しない。text 一致だけでの dedup は行わない。
- **mirror は merge せず、物理行をすべて保持する**（最終決定）。API 表現と terminal 表現が
  同一発言を指す場合も、それぞれが自分の `source`（raw line / byte 範囲）と `event_id` を持ち、
  `mirror_of`（対応する origin id）と `mirror_relation` で関係を注記する。物理行を落とす merge は
  行わないため、`count` で pointer の代用をしない。
  実装は [src/query/normalize.rs](../../src/query/normalize.rs) と
  [src/query/output.rs](../../src/query/output.rs)。

### 2.3 filter / context / OR 検索

- `--since` は境界を含み `--until` は含まない（offset 付き RFC3339、`parse_rfc3339_ms`）。
  時刻欠損は unknown として数え、mtime で補完しない。原文順を保持し、逆行 timestamp で並べ替えない。
- `--query` の繰返しは OR。各ヒットは `matched_queries` を持ち、`matched_ranges` は
  **原文 text に対する UTF-8 byte offset**。表示 cut 後も range は原文基準。
- `--context K` は hard scope 内の前後 K event。soft filter を越えた隣接 event は
  `is_context: true` で明示し、重複は event id で1回に畳む。
- **primary が context を兼ねる場合は primary を優先**する（同一 event を
  primary と context の両方として二重に扱わない）。

### 2.4 tool 対応

- 対応付けは `provider + session_id + call_id` の範囲のみ。近い時刻という理由で結ばない。
- 状態は次の5つを区別する。

  | state | 意味 |
  |---|---|
  | `paired` | call と結果が揃っている |
  | `pending_result` | snapshot 内に結果が無いことが**確認できた** |
  | `unknown_pair` | snapshot を最後まで走査できず、未着と断定できない（pair 走査が incomplete / gap） |
  | `orphan` | 結果だけがあり、対応する call が無い |
  | `not_rendered_budget` | 対応は存在するが表示予算で出せない |

- `unknown_pair` は「未着」ではない。走査範囲に gap や未検査 record がある場合、
  `pending_result` と混同しない。pair 走査自体が不完全なら `scan.with_pair.incomplete: true`。
- 逆に、終端がまだ書き込み中で newline が無い **PendingTail は pending_result のまま**であり、
  `incomplete` を立てない（「未完の末尾」と「証明の欠落」は別）。
- 対応件数は `paired_total`（実総数）と `paired_event_ids`（保持した ID）を分けて返す。
  保持上限を超えた場合は `paired_ids_truncated: true` とし、`pair_reentry`
  （未保持 counterpart の raw line / byte 開始）で再入できる位置を示す。
  そのため「配列が短い = 相手が少ない」と読み替えない。
- 元の `is_error`・出力本文・rc はそのまま保持し、**CLI が依頼作業の完了を推測しない**。
- `--with-pair` は選択済み call の counterpart を最大2パスの追加走査で集める（ADR 0002 §5）。

### 2.5 出力予算

- stdout + stderr の合計が `--max-output-bytes`（既定 32768、最小 8192）に収まる。
- 描画前に metadata を予約し、本文は UTF-8 境界で切って `text_complete: false` を立てる。
  未描画の primary は消費せず、`page_cursor` をその位置に置く。
- 予算内に収まらない場合は `budget_too_small` の rc2（bounded 診断）で拒否し、
  途中までの不正 JSON を出さない。

## 3. 比較した代替案

| 代替 | 利点 | 採らなかった理由 |
|---|---|---|
| 既存 export + 外部 Python/シェル | 実装ゼロ | 毎回の全量再出力、位置の復元不能、provider 差の扱いがスクリプトごとに散る（U02/U03/L01） |
| legacy 抽出器を新 schema へ書き換え | コード一本化 | F09 の互換契約を壊す。既存利用者の schema・終了コードが変わる |
| 常駐 DB / index / TUI | 検索が速い | 常時構築のコストと無書き込み要件（N04）に反する。MVP の範囲外 |
| 新規に別バイナリ | 分離が明確 | 配布・install-skills・埋込 skill が二重化する |

## 4. 帰結

- 良: legacy 利用者は無変更。閲覧系は stdout 完結・無書き込みで、agent の context 予算に収まる。
- 良: raw 位置（line / byte / subevent）を常に返すため、表示が cut されても原文へ戻れる。
- 負: 2つの schema と2つの終了コード契約を保守する。README で両者を明示する必要がある。
- 負: `--with-pair` は追加走査を行う（読取 byte が増える）。計測で別掲する。
- 負: cursor は append-only 前提で、可視範囲外の prefix 書換えまでは検出しない（ADR 0002 §4）。

## 5. 証拠

合成 fixture による実バイナリ subprocess E2E（[tests/viewer_e2e.rs](../../tests/viewer_e2e.rs), [tests/viewer_integrity_e2e.rs](../../tests/viewer_integrity_e2e.rs)）。実会話・実 home は使わない。

| 要件 | 主なテスト（`tests/viewer_e2e.rs`） |
|---|---|
| F01 | `a01_sessions_discovers_exact_cwd_started_old_updated_recent`, `a01_sessions_ambiguous_duplicate_id_fails_rc1`, `a01_sessions_default_roots_resolve_under_isolated_home` |
| F02 | `a02_provider_bodies_and_phase_and_framework`, `a03_time_boundaries_unknown_retrograde_and_tail`, `f02_private_channel_message_is_hard_scope_excluded` |
| F03 | `a04_search_or_queries_context_and_hard_scope`, `f03_context_never_crosses_hard_scope_kind` |
| F04 | `a05_tool_pairs_multiple_parallel_custom_claude_pending_and_budget`, `f06_paired_append_renders_absolute_raw_order`, `f_aud02_uninspected_result_is_unknown_pair_not_pending`, `r02_counterpart_total_is_exact_with_truncated_ids`, `cap_preceding_counterpart_does_not_block_append_progress`, `event_lookup_respects_scope_and_privacy_and_pairs`, `viewer_integrity_e2e::f04_pending_call_at_snapshot_ignores_result_appended_later` |
| F05 | `a06_provenance_subevents_unknown_and_cut`, `f05_source_generation_stable_on_append_changes_on_replace` |
| F06 | `a07_cursor_pages_through_subevents_then_appends`, `a08_invalid_cursors_are_typed_errors`, `f06_append_between_pages_preserves_old_snapshot_then_append_reads_new`, `f06_search_cursor_reuses_query_and_rejects_conflicting_query`, `f06_cursor_truncation_mid_line_is_truncated`, `f06_cursor_explicit_filters_omitted_vs_explicit`, `f05_source_generation_stable_on_append_changes_on_replace`, `viewer_integrity_e2e::f06_initial_snapshot_holds_while_append_arrives_then_append_reads_it`, `event_absent_only_when_snapshot_fully_inspected`, `f06_cursor_context_profile_is_inherited_and_conflicts_rejected`, `f06_cursor_regex_mode_is_inherited_independently_of_query` |
| F07 | `a09_budgets_record_limit_and_retry`, `f07_retry_cursor_reads_rejected_row_without_prefix`, `f_aud01_max_events_never_drops_hits_without_cursor`, `cap_leading_context_cannot_starve_a_hit` |
| F08 | `a10_pipe_empty_error_and_broken_pipe` |
| F09 | `a11_legacy_outputs_and_exit_codes_stable`, `a11_installer_matrix_nine_cases`, `a11_version_consistency` |
| F10 | `a12_integrity_bad_middle_pending_tail_and_unknown_format`, `viewer_integrity_e2e::a12_actual_mid_read_mutation_is_detected`, `a12_unchanged_source_scans_completely` |
| 横断 | `r01_time_exclusion_is_not_role_exclusion`, `f02_private_channel_message_is_hard_scope_excluded`, `f03_context_never_crosses_hard_scope_kind` |

再現:

```bash
rtk proxy cargo test --test viewer_e2e
rtk proxy cargo test --test viewer_integrity_e2e
rtk proxy just check
```

**状態の区別**: 上記テストは**この検証記録の時点で局所実行した**範囲を示す。
CI（fmt/clippy/test）は merge の必須ゲートであり、release は tag 駆動である。
通過・公開の有無は時点で変わるため、本書は固定の「未実施」を主張せず、
[GitHub Actions](https://github.com/yuki-inaho/agent-jsonl-compact/actions) と
[Releases](https://github.com/yuki-inaho/agent-jsonl-compact/releases) を現在状態の正とする。
本書が主張するのは「この記録時点の局所証拠」までである。

## 6. 限界と後続

- 性能の一般化は主張しない。計測は合成入力（seed 固定）に対する限定値である（ADR 0002 §6）。
- Harness² は「proposal → counterexample → composition」という進め方の参照であり、
  native 比較実験の完了を意味しない。MVP の DoD に A/B 実験は含めない。
- F11（follow）/ F12（親子・fork 候補）/ F13（stdin・範囲 export・cache）は P1 として分離し、
  本 ADR の完了範囲に含めない。
