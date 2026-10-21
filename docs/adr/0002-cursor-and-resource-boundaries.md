# ADR 0002 — cursor と資源境界

- Status: **Accepted / Implemented / 本記録は局所検証（local evidence）まで。**
  CI・release の現在状態は [GitHub Actions](https://github.com/yuki-inaho/agent-jsonl-compact/actions) /
  [Releases](https://github.com/yuki-inaho/agent-jsonl-compact/releases) を参照。
- Date: 2026-10-21
- 関連: [ADR 0001](0001-additive-session-viewer-cli.md), `src/query/cursor.rs`, `src/query/reader.rs`, `src/query/runner.rs`

## 1. 問題

追記され続ける会話ログを、毎回先頭から読み直さずに読むには「どこまで読んだか」を
外部へ持ち出す必要がある。同時に、巨大 record と巨大出力でメモリと context が壊れない
ようにする必要がある。素朴な実装は次のどれかで失敗する。

- 既読位置だけを保存する → 途中ページや subevent の続きを取りこぼす / 二重に返す
- 全 prefix の hash を毎回取る → 追記のたびに全量読みになり差分の意味が消える
- cursor を信用する → 改竄・別 file 流用・切詰めで無言の誤読が起きる

要件は F06（差分 cursor）、F07（件数・byte 上限）、N01（全件非保持）、N02（prefix 再読しない）、
N04（無書き込み）、§5 のデータ契約。

## 2. page cursor と append cursor

**2種類を明示的に分ける。** 混用は `cursor_invalid` / reason `wrong_cursor_kind`。

| | page cursor | append cursor |
|---|---|---|
| 意味 | 固定 snapshot 内の続き | 走査完了時点の完全行末から、その後の追記 |
| 保存する位置 | raw 行開始 byte + 次の subevent index + raw line | `complete_line_end_offset` + `complete_line_count` |
| 発行条件 | 未描画の primary が残っている | snapshot 内を完全走査し、未検査 gap が無い |
| 禁止 | 全 subevent 消費前に checkpoint を進める | 途中ページで発行する / 未検査 record を飛ばして前進させる |

実装: `src/query/cursor.rs` の `CursorKind` と `CursorPayload`。payload は
`v`, `kind`, `provider`, `source_id`, `generation{dev,inode}`, `snapshot_end`, `offset`,
`raw_line`, `subevent_index`, `complete_line_count`, `filters`, `event_filter`, `command`,
`session_id`, `queries`, `regex`, `context`, `anchor_hex`, `resume_anchor_hex`, `anchor_len`。
token は `c<version>:` 接頭辞 + base64url(JSON)（v1 では `c1:`）で、
**不透明・版付き・サイズ有界**として扱う。

### 1 raw → 複数 event

Claude の1物理行が複数の text subevent に展開される場合、page cursor は
**raw 行の開始 byte + 次の subevent index** を保存する。表示上限が raw の途中に当たっても、
次のページで未出力 subevent を飛ばさず、既出 subevent を再送しない
（`tests/viewer_e2e.rs::a07_cursor_pages_through_subevents_then_appends`）。
subevent index は**一意・安定・raw 順**であればよく、可視集合が連続であることは要求しない
（private block を正規化段階で別 index に割り当てる実装があり得るため）。

### first-unrendered resume

描画は metadata を先に予約し、本文を UTF-8 境界で切る。**未描画の primary は消費せず**、
最初の未描画 primary の位置に page cursor を置く。`selected` / `rendered` / `not_rendered`
を別々に返す（`src/query/runner.rs` の metadata）。

## 3. 意味的 filter の再利用と「明示 vs 省略」

- cursor は発行時の `filters`・`event_filter`・`queries`・`regex`・`context`・`command` を保持し、
  再開時に自動で再利用する。`--query` を省略した search 再開は元の query で評価する
  （`f06_search_cursor_reuses_query_and_rejects_conflicting_query`）。
- 明示的に**異なる** filter を渡した場合は `cursor_invalid` / `filter_mismatch`（rc1）。
  同じ値を明示した場合は競合ではない。
- 既定値と「明示された既定値」を区別するため、比較は clap の値ソース/`Option` で行い、
  「既定値と等しいから同じ」とは判定しない（`f06_cursor_explicit_filters_omitted_vs_explicit`）。
- metadata の `windows.context` は**実効値**（cursor から継承した値を含む）を示す。
  再開時に `--context` を省略しても 0 に戻らず、その値で前後 context を評価する。
  異なる値を明示した場合のみ `filter_mismatch`。
- `tail`・`limit`・`max-events`・`max-output-bytes`・`max-record-bytes` は**表示・資源条件**であり
  意味的 fingerprint に含めない。したがって tail 表示のあとでも同じ filter で追記を読める。
  record 上限を上げる再試行も同様に許可される。

## 4. source 同一性・anchor・再現性

| 検出 | 手段 | 返す状態 |
|---|---|---|
| file 置換 | `dev`+`inode` の不一致 | `cursor_invalid` / `generation_mismatch` |
| 切詰め | 現在長 < `snapshot_end` | `cursor_invalid` / `truncated`（行途中の切断も同じ） |
| 同一 inode の境界書換え | checkpoint 直前 up to 128 byte の SHA-256 anchor と snapshot 境界 anchor | `cursor_invalid` / `anchor_mismatch` |
| 読み取り中の観測可能な変更 | anchor/metadata の再確認 | `status: partial` + `source_changed: true`（rc1） |
| 非 Unix platform | 安定した世代識別が無い | `cursor_unsupported_platform`（rc1） |

- **append-only 前提**である。anchor は局所境界の一致を示すだけで、可視範囲外の prefix 全体の
  不変性を暗号学的に保証しない。厳密な再現が必要なら固定 snapshot / hash 付き export を使う。
- cursor は検証対象であり信頼対象ではない。範囲外 offset・不正 subevent・壊れた token は
  fail closed する（`a08_invalid_cursors_are_typed_errors`）。
- 無言の先頭再読・別 file 追従はしない。
- 無書き込み: query 系は cache/index/state file を作らない（`a12_integrity_*` の不変条件）。
  読み取りは source の bytes・長さ・mtime を変えない（atime は保証対象外）。

## 5. record 上限・出力予算・「読取 byte」の意味

### 入力側（record 上限）

- `--max-record-bytes`（既定 1 MiB）を超える行は `Uninspected` として byte 範囲のみ記録し、
  `status: partial` / rc1、`append_cursor` は発行しない。`counts.uninspected.samples[].retry_cursor`
  が**実際に拒否した位置**を指し、より大きい上限で同じ raw 開始位置から再読できる
  （`a09_budgets_record_limit_and_retry`, `f07_retry_cursor_reads_rejected_row_without_prefix`）。
- 未終端の末尾は `pending_tail: true` として待機扱いにし、途中の不正行とは区別する。

### 出力側（budget）

- `--max-output-bytes`（既定 32768、最小 8192）は **stdout + stderr の合計**。
  metadata・title・診断・改行を含む。極小値は読み取り前に rc2 で拒否する。
- 全 event + 最後の metadata が予算に収まるよう、描画前に metadata を予約する。
  収まらない場合は `budget_too_small` の rc2（bounded 診断）で拒否し、
  切り詰めた不正 JSON を出さない。
- `--max-events`（既定 200、上限 10000）は primary + context + pair の**総表示 event 上限**。
  予算で表示しなかった pair は `not_rendered_budget` として `pending_result` / `unknown_pair`
  と区別する。**キーの存在ではなく、実際に emit された event の `pair_state` と
  非空 `paired_event_ids`、または正の構造化カウントで判定する**（監査の positive gate）。

### pair 走査の状態

- 走査が最後まで完了し、gap も未検査 record も無い場合だけ、結果の不在を `pending_result` と
  断定できる。
- 走査が不完全（未検査 record、途中の invalid、snapshot 到達前の停止）なら、不在は
  `unknown_pair` として返し、`scan.with_pair.incomplete: true` を立てる。
  「未着」と「未走査」を混同しない。
- 一方、snapshot 末尾がまだ書き込み中で newline が無い **PendingTail は pending_result** と
  して扱い、`incomplete` を立てない。未完の末尾は「証明の欠落」ではない。
- 1 event あたりに保持する counterpart ID は上限を持つ。実総数は `paired_total`、
  保持した配列は `paired_event_ids`、打ち切りは `paired_ids_truncated`、
  再入位置は `pair_reentry`（raw line / byte 開始）で示す。
- 表示予算で出せないだけの counterpart は `not_rendered_budget` であり、
  pending でも unknown でもない。

### 読取 byte の2つの値

| field | 意味 |
|---|---|
| `scan.read_bytes` | 主走査パスで論理的に消費した source byte |
| `scan.total_read_bytes` | 主走査 + 補助走査（format 判定・metadata 探索・anchor 検証・pair パス）の合計 |
| `scan.read_bytes_breakdown.{main,detect,metadata,anchor,validation,pair}` | 上記の内訳 |
| `scan.with_pair.passes` / `scan.with_pair.read_bytes` | pair 追加走査のパス数と読取量 |

いずれも**論理消費 byte**であり、OS の物理 I/O でも「速さ」でもない。`BufReader` の先読みは
含めない。補助走査があるため `total_read_bytes` はファイルサイズを超え得る。
`-i` と cursor の組み合わせでは cursor から provider / session を再利用し、旧 snapshot の
anchor 近傍と追記分だけを読む。ただし `--with-pair` 指定時は対応付けのため同一 snapshot を最大2回
追加走査するので、この限りではない（計測表の pair read を参照）。一方 `--session` は cursor を
使う前にも catalog 探索を行うため、呼び出しごとにその探索コストを負担することがあり、追記分だけの
O( suffix ) を保証しない。

## 6. 計測（限定・再現可能）

`scripts/benchmark_viewer.py` が合成入力を生成し、release バイナリを実行して
JSON receipt と Markdown 要約を出す。**この helper が公開できる証明**であり、
作業用の一時ディレクトリの記録を根拠にしない。

```bash
cargo build --release
python3 scripts/benchmark_viewer.py generate --out temp/viewer_bench/input.jsonl
python3 scripts/benchmark_viewer.py measure \
  --input temp/viewer_bench/input.jsonl --out-dir temp/viewer_bench/out
```

receipt には次を残す（helper が出力する）。

| 群 | 項目 |
|---|---|
| 入力 | seed、生成パラメータ（provider 比率・本文長・3-subevent 比率）、record 件数、`input sha256` |
| バイナリ | `binary_sha256`、debug/release、`rustc --version`、platform |
| 計測 | `read_bytes`（主走査・logical）、`total_read_bytes` と breakdown、`stdout_bytes` + `stderr_bytes`、wall、peak RSS |
| 条件 | OS cache は cold/warm を制御していない（値はラベルであり保証ではない） |

計測値（**context / regex プロファイル修正前の計測候補** release バイナリ `bc52b88d…` の
receipt。最終凍結 source の検証済み候補**バイナリ**は sha256 `7ee5e42b…`（source 側は runner
`61f297b5…`）であり、この計測の対象ではない）:

- 入力: seed 42、`claude_code`、physical records 110,088、bytes 69,234,347、
  input sha256 `0c64ec59841f8f212abb42498ec3c6670fe91f6dbbd72fa2fe370acd7a74f39a`
- shape: 80% single text / 10% tool_use+result / 10% 3-text block、body 256B（1% は 16,384B）
- binary: `bc52b88d217e903ff62c80dd75bce81aeeaefa9363c721683ce824bce54ed3ea`（release, 3,819,816 B）
- 環境: linux / python 3.10.6 / OS cache は cold-warm 未制御

| ケース | 主走査 read | total read | wall | peak RSS | 出力 |
|---|---|---|---|---|---|
| view 先頭50件 | 23,603 B | 174,590 B | 0.0041 s | 4,652 KiB | 29,209 B |
| search 全体・末尾ヒット1件 | 69,234,347 B | 69,385,462 B | 0.6577 s | 4,736 KiB | 3,076 B |
| view --scope tools --with-pair | 179,464 B | 138,799,145 B | 2.1657 s | 4,736 KiB | 30,848 B |
| append（3行追記） | 579 B | 1,413 B | 0.0027 s | 4,268 KiB | 3,853 B |

- 数値は**論理的に消費した source byte**であり、OS の物理 I/O でも速度の一般化でもない。
- `--with-pair` の total read が大きいのは、設計上の最大2パス（counterpart 収集 + 描画）による。
- cold/warm は不明。debug/release の別と hardware は receipt に従う。
- **この表を根拠に「速くなった」とは主張しない。** 比較実験（Harness² の native A/B）は
  MVP の DoD に含めていない。
- 母集団は合成入力1種のみ。実会話ログや 1 GB 級への一般化はしない。

## 7. 帰結と限界

- 良: 追記読取りが既読 prefix の再読を要求しない。append の読取量は追記分に比例する。
- 良: 未検査 record・未完末尾・不正行・形式未確定が、すべて別の状態として観測できる。
- 負: cursor が長い（base64 で数百〜数千 byte）。意味的 filter の合計は 2048 byte を上限とする。
- 負: append-only 前提のため、可視範囲外の書換えは検出できない。厳密監査には snapshot 保存が要る。
- 負: 非 Unix では cursor モードを使えない（明示拒否）。
- 後続（P1）: `follow`（F11）、親子/fork 候補（F12）、stdin・範囲 export・cache（F13）。

## 8. Status の内訳

- Accepted: 承認済みの設計判断（要件 §4–5 と本 ADR の決定）。
- Implemented: `src/query/{reader,cursor,runner,select,toolpair,source}.rs` に存在。
- Locally validated: 上記 named tests と限定計測が局所で実行済み。
- **CI / release の状態は本書に固定しない。** 本書の記録は局所検証までであり、
  merge には CI が必須ゲート、release は tag 駆動である。現在状態は GitHub Actions / Releases を参照。
