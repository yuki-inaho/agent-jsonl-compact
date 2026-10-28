# 変更履歴

[English](CHANGELOG.md) | **日本語**

`agent-jsonl-compact` の主な変更を記録します。英語版を正本とし、本書は日本語版です。

## 0.5.1 — 2026-10-28

### 変更

- 移植作業記録のローカルパスを一般化しました。この版には、Releaseを公開しなかった0.5.0の変更も含まれます。
- インストール時の指定、探索先、版の情報、カーソルの条件について、文書の説明を明確にしました。

## 0.5.0 — 2026-10-28

Source release date: **2026-10-28**（固定のリリース日。build 時刻ではありません）。

### Breaking

- `event_id` を `source_id:byte_start:subevent_index` に変更し、未検証の行番号を識別子から除外した。旧形式の `--event` ID と、旧 cursor が返した event_id は再取得できない。
- 閲覧JSONLに `source.raw_line_verified`、`matched_ranges_total`、`matched_ranges_truncated`、`pair_reason`、`pair_state=ambiguous` を追加した。既存の5つの pair state は維持する。
- 1レコードの正規化に上限を設けた。timestamp 128 byte、識別子 512 byte、phase 64 byte、subevent 256 個、正規化後 2 MiB を超える record は `parse_error` と `status: partial` になる。従来読めていた過大な record は拒否される。
- ソースビルドの最低要件をRust 1.88へ変更。`time` と `anyhow` をRustSecの修正版へ更新。
- `install-skills` は既存ファイルを保持。確認後に `--force` で置換。symlinkで登録したスキルは拒否。
- 抽出名のパス区切り・制御文字、入力と同じ実体、通常ファイル以外への出力を拒否。

### セキュリティ

- **F-01 配布:** v0.4.0 に含まれていなかった安全な保存・導入処理を 0.5.0 に含める。出力名の検証、symlink・リンク先・入力自身の拒否、同一ディレクトリの一時ファイルからの置換、Unix 0600、既存スキルの保持、installer の tag・checksum・archive 検査を維持する。`version --output json` に `source_commit` と `source_commit_dirty` を追加し、release workflow は同じコミットの CI と security の成功を必須にする。
- **F-02 開発補助:** `just stats` は `set positional-arguments` と `"$1"` を使い、ファイル名をシェル文へ埋め込まない。
- **F-03 正規化:** 長い timestamp・識別子と多数の subevent を draft 生成前に拒否し、1 MiB 未満の入力からのメモリ増幅を抑える。
- **F-04 表示:** timestamp・ID・見出し・診断を含む外部由来表示を `escape_controls_singleline` に通す。本文は改行を保持し、JSON の内部値は原値のまま。
- **F-05 資源:** 検索一致は最大256件で打ち切り、`matched_ranges_truncated` を返す。primary を1件も描画できない場合は rc0 の同一 cursor 反復ではなく rc2 `budget_too_small` にする。context は絶対保持上限を守り、FIFO・ソケット等の非通常ファイルを拒否し、探索にエントリ数・深さ・読取 byte・処理時間の上限を設け、検索は30秒の処理予算を持つ。
- **F-06 対応付け:** provider・session・call_id を構造化キーにし、call と result を型別に照合する。別セッション、区切り文字入り ID、重複 ID、call のみ、result のみを区別し、重複は `ambiguous` とする。
- **F-07 継続:** cursor の token・配列・文字列・整数・位置関係に上限と整合性検査を追加。session 名を原文と照合し、行番号を event_id に使わない。極端な整数でも panic しない。
- **F-08 探索:** scan limit・読取不能・未検査・不正 record が残る場合、`view --session` / `search --session` は `session_resolution_incomplete` を返し、一意性や不在を断定しない。解析状態は候補採用より先に集計する。
- **F-09 抽出:** legacy 抽出で `isMeta` を `injected` / `origin=framework`、`isCompactSummary` を `compacted` / `origin=compaction` として保持する。Markdown の会話本文は fence 内に置き、本文由来の見出し・外部画像を生成側の構造と混同しない。ログを現在の権限・承認・副作用許可へ変換しない契約を README / SECURITY に記載する。
- 非公開権限の一時ファイルから抽出物を保存。リンク先をたどらず既存ファイルを置換。統計・レポート・エラーの端末制御文字をエスケープ。
- 導入時のタグ・checksum・アーカイブのパスと種別を検査。検証後に配置し、取得したバイナリは自動実行しない。
- 日次検査と配布前のsecurity gate、ActionsのSHA固定、配布権限の縮小、イベント値のシェル埋込み除去。
- `release-verify` は `release` workflow の成功後にも実行する（`workflow_run`）。`GITHUB_TOKEN` で作成した Release は `release: published` を発火しないため。

### 変更

- 配布 tarball に README.en.md、SECURITY、CHANGELOG、docs を同梱し、展開後の README 相対リンクを解決する。
- 日英の案内を短縮し、CLI詳細と入力形式の根拠を参照文書へ移動。CLIリファレンスに `source_commit`、`raw_line_verified`、一致省略、pair `ambiguous`、`session_resolution_incomplete`、探索上限を追記した。
- `version --output json` の `source_commit` と `source_commit_dirty` を増分ビルド（linked worktree を含む）でも最新の Git 状態へ更新し、Git を利用できない場合は dirty を `null`（不明）として clean と区別する。
- 新しい設計判断を ADR 0007 に記録した。

## 0.4.0 — 2026-10-23

Source release date: **2026-10-23**（固定のリリース日。build 時刻ではありません）。

### Breaking

- 抽出の既定出力を `<name>.clean.jsonl` のみに変更。`<name>.transcript.md` は
  `--format-out md|both`、`<name>.summary.json` は `--summary` を指定したときだけ出力する。
  clean schema、その他の抽出オプション、終了コードは不変。

### 追加

- summary JSON を明示的に出力する `--summary` フラグ。
- macOS native E2E: `macos-latest`（arm64）と `macos-15-intel`（x86_64）の両方で
  `cargo test` を実行し、続けて release バイナリの smoke test を行う。
- `tests/fixtures/claude_desktop_sample.jsonl`: sanitize 済みの実 Claude Code / Desktop
  transcript。text、thinking、tool call/result、末尾の pending tool call を含む。
  由来と sanitize 方針は `tests/fixtures/README.md` を参照。
- `scripts/smoke-release.sh`: release バイナリに対して `--version`、`version --output json`、
  `--stats`、`view`、`search`、抽出を実行する。
- `release-verify.yml`: 公開済みの Linux/macOS release asset を download し、checksum を
  検証したうえで native runner 上で release バイナリを実行する。

### 変更

- release workflow は Intel macOS asset を `macos-15-intel` で native build し、
  packaging 前に release バイナリの smoke test を実行する。
- `weekly-release-check.sh` は tests/docs/CI のみの変更をリリース必須と判定しない。

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
