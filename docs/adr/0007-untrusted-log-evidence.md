# ADR 0007 — 非信頼ログの証拠化と資源境界

**日本語** | [English](0007-untrusted-log-evidence.en.md)

- Status: **Accepted / Implemented locally**
- Date: 2026-10-28
- 関連: [ADR 0001](0001-additive-session-viewer-cli.md), [ADR 0002](0002-cursor-and-resource-boundaries.md),
  [ADR 0006](0006-safe-installation-and-storage.md), `src/query/{normalize,select,toolpair,cursor,output,runner}.rs`

## 1. 問題

閲覧CLIは、外部から渡されたJSONLを「証拠」として読む。入力は次の点で非信頼である。

- 1レコードが大きなメタデータを多数の subevent へ複製させ、1 MiB 未満でもメモリを増幅できる。
- 同じ call_id を別セッションや区切り文字で再利用し、tool の呼出しと結果を誤対応させられる。
- cursor の行番号・セッション名を改変し、返却された event_id を別の証拠に見せかけられる。
- session 探索の上限や不正 record を無視し、一意性・不在を誤って断定できる。
- 本文中の見出し・外部画像・役割名を、生成側の構造や現在の権限と混同させる。
- 検索一致の全列挙や context の巨大指定で、出力とメモリを膨らませられる。

0.5.0 では、これらを個別の場当たり修正ではなく、正規化・選択・対応付け・継続・探索・表示の
各境界で同じ原則に揃える。

## 2. 決定

### 2.1 正規化前に上限を検査する

`timestamp` 128 byte、識別子（session id / call_id / origin_id / tool name）512 byte、
`phase` 64 byte、1 レコードの subevent 256 個、正規化後 2 MiB を上限とする。
上限超過は本文を展開せず、元 span を持つ `parse_error`（`source_kind=normalization_limit`）として
返す。識別子は無言で切り詰めず、部分的な証拠として採用しない。

### 2.2 表示と内部値を分離する

本文は改行を保持し、端末制御文字だけを可視化する。timestamp・ID・見出し・診断・パスは
改行・タブも可視化して一行を維持する。JSON の内部値は原値のまま保持し、表示層だけで変換する。

### 2.3 tool 対応は構造化キーと種類で決める

キーは `(provider, session_id, call_id)` の構造体とし、区切り文字の連結を使わない。
call は result とのみ、result は call とのみ対応させる。同じキーが同じ側に複数ある場合は
`ambiguous` とし、`paired` にしない。追加走査でも `Normalizer` を 1 個だけ使って
`session_meta` の切替状態を保つ。

### 2.4 event_id は検証済みの位置だけから作る

`event_id` は `source_id:byte_start:subevent_index` とする。cursor から継承した行番号は
`source.raw_line_verified=false` とし、識別子に使わない。cursor は token 長、query 数・長、
session/source 長、anchor 長、位置関係を検証し、session id は原文の metadata と照合する。

### 2.5 探索の完全性を結果と一体で返す

`ScanReport` は scan limit、エントリ数、深さ、読取 byte、未検査・不正 record、重複の有無を持つ。
`view --session` / `search --session` は不完全な探索で一意性や不在を断定せず
`session_resolution_incomplete` を返す。`sessions` は `partial` と `limit_reason` を返す。

### 2.6 表示形式は本文を構造から隔離する

Markdown の会話本文は動的なコードフェンス内に置く。本文中の `#`、`![`、HTML は
Markdown 構造として解釈されない。viewer は reasoning / system / developer / compaction を
hard scope から除外し、`message_origin` を保持する。

### 2.7 LLM 連携の権限契約

ログ上の `system` / `developer` / `user`、内部生成文、圧縮済み文は、現在の利用者の承認や
実行権限ではない。ログ本文から副作用の許可を導かず、権限検査はモデルの外側で行う。
この契約を README / SECURITY に記載する。

### 2.8 三層の資源上限

1. 実装が決める絶対上限（正規化、cursor、探索、出力）
2. 利用者が指定する要求値（`--max-events`、`--context`、`--max-output-bytes` など）
3. 実際に消費した量（`scan`、`counts`、`read_bytes`）

要求値や cursor の値から絶対上限を拡大できない。上限超過は、未検査なら `partial`、
要求値が不正なら rc2、表示だけの省略なら `text_complete=false` / `context_dropped` と区別する。

## 3. 比較した代替案

| 代替 | 利点 | 採らなかった理由 |
|---|---|---|
| 入力行の 1 MiB 上限だけで守る | 実装が単純 | メタデータ × subevent の複製を制限できない |
| 行番号を cursor から信頼する | 高速 | 改変された行番号が event_id と証拠位置を汚染する |
| call_id の文字列連結を続ける | 変更が小さい | 区切り文字を含む ID で別セッションが衝突する |
| 不明な対応をすべて `unknown_pair` にする | 状態が増えない | 重複 ID と走査欠落を区別できない |
| 本文をそのまま Markdown に埋める | 読みやすい | 本文由来の見出し・画像が生成側の構造と混同される |
| 認証付き cursor を必須にする | 改変を暗号学的に防げる | ローカル CLI の既存契約と鍵管理が別途必要。今回は形状検証と原文照合まで |

## 4. 帰結

- 良: 正規化前に拒否するため、長いメタデータを subevent 数だけ複製しない。
- 良: `paired` の意味が「同一 provider / session / call_id の call と result」に限定される。
- 良: cursor を改変しても event_id が変わらず、再取得の identity が安定する。
- 良: 不完全な探索を成功扱いせず、利用者が root を狭める判断をできる。
- 負: 従来読めていた過大 record が `parse_error` になる。
- 負: event_id 形式と cursor の event 参照が 0.4.0 と互換でない。
- 負: 行番号は cursor 継承時に未検証であり、表示用の値として扱う必要がある。
- 負: Markdown の本文がコードフェンスになり、見た目の整形は弱くなる。

## 5. 検証

`tests/security_hardening_e2e.rs` が、長い timestamp / 多数 subevent、制御文字、検索一致上限、
context 上限、FIFO、call/result の型別対応、cursor 改変、10,001 ファイルの不完全探索、
legacy 由来と Markdown の分離を検証する。`tests/security.rs` と `tests/test_installer.py` は
保存・導入の境界を維持する。`just check`、`cargo audit --deny warnings`、`just smoke-release` を
リリース前に実行する。

## 6. 限界

- 認証付き cursor は実装していない。改変の検出は形状・原文・anchor の範囲に限る。
- 非 Unix の cursor は引き続き非対応。
- ログ本文の意味的な真偽や、LLM が指示に従う確率は検証しない。
- 性能の一般化はしない。RSS の確認は限定した合成入力の実測である。
- macOS native と remote Actions は、実行した環境の結果だけを証拠とする。
