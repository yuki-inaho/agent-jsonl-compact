# 変更履歴

[English](CHANGELOG.md) | **日本語**

`agent-jsonl-compact` の主な変更を記録します。英語版を正本とし、本書は日本語版です。

## 0.3.1 — 2026-10-23

Source release date: **2026-10-23**（固定のリリース日。build 時刻ではありません）。

### 追加

- macOS の prebuilt 成果物: `aarch64-apple-darwin`（Apple Silicon）と
  `x86_64-apple-darwin`（Intel）。各 `.tar.gz` + `.sha256` を公開。
- `install.sh` が Linux x86_64 musl と macOS arm64/x86_64 を判定し、
  `sha256sum` または macOS の `shasum -a 256` で checksum を検証。

### 変更

- `release.yml` を Linux/macOS の matrix build に変更し、全資産を 1 つの GitHub Release へ公開。
- CI が push / pull request ごとに macOS 2 target を build。
- [ADR 0004](docs/adr/0004-versioning-and-release-policy.md)（バージョニングとリリース方針）と
  [ADR 0005](docs/adr/0005-bilingual-documentation.md)（二言語ドキュメント方針）を追加。
- README / ONBOARDING / RELEASING / ADR 索引を日英ペア化。日本語をデフォルトとし、
  英語版は `*.en.md` に配置。
- `just weekly-check` / `scripts/weekly-release-check.sh` が最新タグ以降の差分と
  リリース要否を表示。`just docs-check` が日英ペアを検査。

### 注意

- CLI の挙動、出力 schema、cursor version、query schema は 0.3.0 から変更なし。
- macOS バイナリは toolchain による ad-hoc 署名のみで、notarization は未実施。README を参照。
- v0.3.0 の後に merge された `SKILL.md` の更新は 0.3.1 のバイナリに埋め込み済み。

## 0.3.0 — 2026-10-21

Source release date: **2026-10-21**（固定のリリース日。build 時刻ではありません）。

### 追加

- `sessions` — 宣言された provider root 配下から会話候補を探索
  （`--root`、`--provider`、`--cwd`、`--cwd-children`、`--scan-limit`）。
- `view` — session または file を bounded 出力で読む。filter
  （`--scope`、`--role`、`--phase`、`--since`、`--until`）、`--tail`、`--event`、
  `--with-pair`、page/append cursor（`--cursor`、`--after-cursor`）に対応。
- `search` — literal（OR）または明示的な `--regex` 検索。`--context` に対応し、
  原文に対する `matched_queries` と `matched_ranges` を返す。
- `version --output json` — 機械可読な version、固定 `source_release_date`、
  query schema version。
- 閲覧系出力は provenance を持つ: raw line、byte range、subevent index、provider
  session id、message origin、tool pair 状態、coverage counter。

### 互換性

- legacy 抽出（`-i/-o`、`--format-out`、`--channel`、`--name`、`--stats`、
  `--no-dedup`、`--elide-outputs`、`--msg-chars`、`--out-chars`）は既存の
  schema、既定値、終了コードを維持。
- 新しい閲覧系コマンドは独自の終了契約を使う: `0` 正常処理（正当な 0 件を含む）、
  `1` source/decode/形式未確定/partial/cursor エラー、`2` 引数または query 構文エラー。

### 注意

- query 系コマンドは source を書き換えず、既定では cache や index を作らない。
- メモリは入力 event 数ではなく、要求 window、record 上限、出力予算で制限される。
- cursor モードは Unix の file generation identity（`dev`+`inode`）を必要とする。
  他プラットフォームでは `cursor_unsupported_platform` で拒否する。
