# LLMオンボーディングサマリー

**日本語** | [English](ONBOARDING.en.md)

> このテンプレートは、新任LLMエージェントがプロジェクトに参加する際に共有する初期資料のたたき台です。各項目を埋め、参照元ドキュメントや補足リンクがあれば併記してください。

## 1. プロジェクト概要と目的
- **プロジェクト名称・領域:** `agent-jsonl-compact` — Codex CLI / Claude Code / OpenCode run のセッション JSONL を軽量化する単一 Rust CLI と、それを使うエージェントスキル。
- **最終成果物:** 生 JSONL を自動判定し `*.clean.jsonl` / `*.transcript.md` / `*.summary.json` を生成する単一バイナリ。加えて `sessions` / `view` / `search` の閲覧 CLI、reader スキル(`agent-jsonl-compact-reader`)、Linux x86_64 musl / macOS arm64 / macOS x86_64 の prebuilt(`install.sh` / GitHub Releases)。
- **ビジネス背景・価値:** 巨大なセッションログを生のままコンテキストへ載せず「軽量化 → 段階読み」でトークンを節約する。`slack-knowledge-rag` 等の周辺ツールから PATH 上の CLI として利用される(`just extract-session` / `inspect-session`)。
- **現時点の進捗サマリ:** `v0.3.1`(2026-10-23)リリース済み。`0.3.0` で `sessions` / `view` / `search` と cursor / provenance を追加し、`0.3.1` で macOS prebuilt、release matrix、ADR 0004/0005、日英ドキュメントを追加した。CLI 挙動・query schema v1・cursor v1 は 0.3.0 から不変。`確認済み`: リリース準備時にローカルで `just check`(fmt / clippy `-D warnings` / unit 54 + integration 6 + viewer E2E 36 + integrity 4 = 100 tests)と `just docs-check` を green で通過。CI と Release の現在状態は GitHub Actions / Releases を参照。

## 2. クリティカルな要求・制約
> 「壊してはいけない」品質・仕様ラインを箇条書きで列挙します。
- 既定は**忠実モード**。シグナル(全メッセージ本文・全ツール出力・全ツール呼出)は削らない。lossy(`--msg-chars`/`--out-chars`/`--elide-outputs`/`--channel api`)は明示指定時のみ作用させる。
- 実セッション JSONL と抽出物(`*.clean.jsonl`/`*.transcript.md`/`*.summary.json`)は会話本文・ホームパス等の **PII を含む。コミット禁止**(`.gitignore` 済)。同梱 fixture は合成データのみ。
- JSONL は **streaming load**。生ログ全体をメモリへ載せない(`src/util.rs`)。保持するのは採用後 event のみ。
- 形式自動判定の優先順(`src/detect.rs`)と正規化 event kind / keep set(`src/classify.rs`・`src/clean.rs`)を破壊しない。
- 配布対象は **Linux x86_64 musl 静的 / macOS arm64 / macOS x86_64**。`install.sh` は上記以外の OS/arch を明示エラーにする。
- cursor モードは Unix の file generation(`dev`+`inode`)を使う。非 Unix では `cursor_unsupported_platform` で明示拒否する。
- `SKILL.md` はビルド時に `include_str!` でバイナリへ埋め込み。**スキル本文を変更したら再ビルドが必要**。
- リリースは `v*` タグ → `release.yml`。Linux と macOS 2 target を matrix build し、1 つの Release へ添付する。リポジトリは public。

## 3. 参照すべき合意済み資料
> 新任エージェントが必ず確認すべき一次資料の一覧です。パスと役割を記載します。
| 種別 | ファイル/リンク | 概要・用途 |
|------|------------------|------------|
| 概要・使い方 | `README.md` / `README.en.md` | インストール(Linux/macOS) / 使い方 / 閲覧CLI / オプション / S/N 方針 / 開発レイアウト(日英) |
| ドキュメント索引 | `docs/README.md` / `docs/README.en.md` | すべてのドキュメントの入口(日英) |
| 変更履歴 | `CHANGELOG.md` / `CHANGELOG.ja.md` | 版ごとの変更点と固定 source release date(英語正本 + 日本語) |
| リリース手順 | `docs/RELEASING.md` / `docs/RELEASING.en.md` | 週次チェック、版の決め方、tag / Release / 資産検証(日英) |
| バージョニング基準 | `docs/adr/0004-versioning-and-release-policy.md` | SemVer の適用、0.x の扱い、patch/minor/major の分類(日英) |
| 二言語ドキュメント方針 | `docs/adr/0005-bilingual-documentation.md` | 日本語デフォルト、`*.en.md`、同期と検査のルール(日英) |
| 設計判断 | `docs/adr/0001-additive-session-viewer-cli.md` / `0002-cursor-and-resource-boundaries.md` / `0003-version-and-release-metadata.md` | 閲覧CLIの追加、cursor と資源境界、版・日付・release の契約 |
| ADR 索引 | `docs/adr/README.md` / `docs/adr/README.en.md` | 全 ADR の一覧と英語サマリ(日英) |
| 要件・受入 | `docs/requirements/session_viewer_cli.md` | F01–F10 / N01–N05 と受入ケース |
| 設計・作業記録 | `docs/workdoc_session_jsonl_compactor_rust_port.md` | 移植根拠・モジュール対応・配布/スキル統合(章10)・DOD |
| スキル仕様 | `skills/agent-jsonl-compact-reader/SKILL.md` | reader スキルの段階読みフロー(summary → 必要箇所だけ) |
| OpenCode CLI仕様 | <https://dev.opencode.ai/docs/cli/#run> | `run --format json`、`export`、`db path` の公式CLI契約 |
| OpenCode JSONL実装 | <https://github.com/anomalyco/opencode/blob/v1.18.26/packages/opencode/src/cli/cmd/run.ts> | JSONL外形とstdoutへ出すevent typeの根拠 |
| OpenCode part型 | <https://github.com/anomalyco/opencode/blob/v1.18.26/packages/sdk/js/src/v2/gen/types.gen.ts> | tool state、tokens、cost等のフィールド定義 |
| Codex rollout writer (0.152.0) | <https://github.com/openai/codex/blob/rust-v0.152.0/codex-rs/rollout/src/recorder.rs> | ローカル `codex-cli 0.152.0` と同じタグ。JSONL writer、保存先、`timestamp` / `ordinal` / flattenされた `RolloutItem` の根拠 |
| Codex rollout互換テスト (0.152.0) | <https://github.com/openai/codex/blob/rust-v0.152.0/codex-rs/rollout/src/tests.rs> | `session_meta`、`event_msg`、`response_item` の合成レコードとJSON互換性の実例 |
| Claude Code session保存 | <https://code.claude.com/docs/en/sessions#where-transcripts-are-stored> | `~/.claude/projects/<project>/<session-id>.jsonl`、1行がmessage/tool/metadata JSON objectであること。行内形式は内部形式で可変という制約もここを正本とする |
| Claude Code application data | <https://code.claude.com/docs/en/claude-directory> | `~/.claude` がtranscript等のアプリケーションデータを含むこと、およびPIIとして扱う根拠 |
| タスク定義 | `justfile` | build / test / check / demo / dist / install* / bench-viewer / docs-check / weekly-check |
| テスト資産 | `tests/integration.rs`, `tests/viewer_e2e.rs`, `tests/viewer_integrity_e2e.rs`, `tests/fixtures/*.jsonl` | 抽出・install-skills・閲覧CLI A01–A12・並行読取り整合性の確認(合成 fixture) |
| CI / Release | `.github/workflows/ci.yml`, `.github/workflows/release.yml` | fmt / clippy / test + macOS 2 target build、`v*` タグで Linux/macOS 成果物を公開 |
| インストーラ | `install.sh` | OS/arch 判定、sha256 検証、`~/.local/bin` への配置 |
| 既知課題リスト | GitHub Issues | 専用の課題リストは未整備。現在状態は Issues / Actions / Releases を参照 |

## 4. タスク境界（任せること / 任せないこと）
### 任せるタスク（例）
- 新形式 / 新レコード type の対応(`src/detect.rs`・`src/classify.rs`)。
- CLI option・出力レンダリングの追加改善(`src/cli.rs`・`src/render.rs`)。
- CI・配布・`install.sh`・スキルの改善、テスト追加(`tests/`)。
- ドキュメント更新(README / workdoc / SKILL.md / 日英ペア)。

### 任せないタスク（例）
- 実セッションデータ・抽出物のコミットや外部送信(PII)。
- 忠実モード既定の破壊的変更(既定で本文 / ツール出力を削る等)。
- 無断の public/private 変更、破壊的リリース(既存タグの付け替え等)。
- ライセンス / 著作権表記の無断変更。

## 5. インタラクション方針
- **回答スタイル:** 日本語。技術用語・コード識別子は原語のまま。見出し + 箇条書き。
- **回答手順:** 前提 → 論点 → 提案。変更は最小差分で、検証コマンドの出力を添える。
- **禁止事項・注意:** 未確定を断定しない。`確認済み` / `未検証` / `推定` を区別する。
- **秘匿情報の扱い:** PII・トークン・会話本文を出力やコミットに含めない。抽出物は `temp/` 等の ignore 済みの場所へ。

## 6. 試行タスク（オンボーディング演習）
> 小さな検証タスクを2〜3件記載してください。理解度を確認するために実施します。
1. `just check` と `just demo` を通し、`demo-out/` の3成果物(`*.summary.json`/`*.transcript.md`/`*.clean.jsonl`)を読む(`確認済み`に動作する)。
2. `agent-jsonl-compact -i tests/fixtures/opencode_sample.jsonl --stats` でOpenCodeの自動判定とレコード型分布を確認する。
3. `agent-jsonl-compact view -i tests/fixtures/codex_sample.jsonl --tail 5` と `search -i tests/fixtures/codex_sample.jsonl --query <語> --context 1` を実行し、metadata の `source` / cursor / `status` を読む。
4. `agent-jsonl-compact version --output json` で `version` / `source_release_date` / `query_schema_version` / `cursor_version` を確認する。
5. 一時 HOME で `HOME=$(mktemp -d ...) agent-jsonl-compact install-skills` を実行し、`.claude`/`.codex` 配下への配置を確認する(実 HOME を汚さない)。

## 7. 運用ルール・変更管理
- **ドキュメント更新時の記載ルール:** 変更は `CHANGELOG.md`(英語正本)と `CHANGELOG.ja.md` に版・日付つきで追記する。利用者向けの手順は `README.md` / `README.en.md`、リリース手順は `docs/RELEASING.md` / `.en.md`、設計判断が変わる場合は `docs/adr/` に ADR を追加する。日本語をデフォルトとし、対応する英語版を同じ変更で更新する(ADR 0005)。相対日付は絶対日付に変換する。
- **TBDの扱い:** 未確定は `未確認` と、次に当たるべき一次資料を明記する。
- **リリース手順:** `docs/RELEASING.md` に従う。`Cargo.toml` の version、`src/query/version.rs` の `SOURCE_RELEASE_DATE`、`CHANGELOG.md` / `CHANGELOG.ja.md` を同じ版・日付に更新 → `just check` / `just docs-check` → main へ push して CI green → annotated tag `vX.Y.Z` を push → `release.yml` が Linux x86_64 musl / macOS arm64 / macOS x86_64 を build し、tarball + `.sha256` を 1 つの Release へ添付する。公開済み tag は付け替えない。
- **週次チェック:** `just weekly-check` で最新タグ以降の差分を確認する。コード、テスト、installer、埋め込み `SKILL.md` に変更があれば patch/minor/major を分類してリリースする。docs/CI のみなら次回コードリリースへまとめてよい(ADR 0004)。
- **レビュー/承認フロー:** CI(`ci.yml`: fmt-check + clippy(-D warnings) + test + macOS 2 target build + docs pair check)を green に保つ。変更は動作検証の出力で裏付ける。
- **その他の運用ルール:** 生成物(`target/` / `demo-out/` / `dist/`)は Git 管理外。`install.sh` は sha256 検証付き。

---

### 付録: 参考情報
- **主要リポジトリ/ディレクトリ:** `src/`(CLI 実装), `src/query/`(閲覧CLI), `skills/agent-jsonl-compact-reader/`(スキル), `.github/workflows/`(CI/Release), `tests/`(fixtures + integration + viewer E2E), `docs/`(ADR / 要件 / 作業書 / リリース手順 / 本書), `install.sh`, `scripts/`(週次チェック・docs 検査・計測)。
- **代表的なコマンド:**
  - `just check` / `just docs-check` / `just weekly-check` / `just demo` / `just dist` / `just bench-viewer`
  - `agent-jsonl-compact -i <session.jsonl> -o <out_dir>`
  - `agent-jsonl-compact --stats` / `agent-jsonl-compact install-skills [--claude-only|--codex-only]`
  - `agent-jsonl-compact sessions --provider codex --cwd <repo>`
  - `agent-jsonl-compact view --session codex:<id> --tail 30 --output jsonl`
  - `agent-jsonl-compact search -i <file> --query <word> --context 2`
- **依存ライブラリ:** `anyhow`, `clap`(derive), `serde`, `serde_json`, `regex`, `sha2`, `base64`, `time`(dev: `tempfile`)。詳細と MSRV 1.74 のピン理由は `Cargo.toml` / ADR 0003。
- **連絡先/責任者:** git author `yuki-inaho`(yoshikawa@inaho.co)。GitHub: `yuki-inaho/agent-jsonl-compact`。

> ※テンプレートは必要に応じて拡張・縮退して構いません。記入済みのドキュメントはバージョン管理してください。

---

### 付録: セッション JSONL フォーマット早見

3形式の JSONL は**別スキーマ**。共通点は「1行1 JSON」「`type` と `timestamp` を持つ」程度。

| | Codex CLI rollout | Claude Code transcript | OpenCode run JSONL |
|---|---|---|---|
| 1行の形 | `{type, payload, timestamp}` | `{type, message, sessionId, uuid, ...}` | `{type, timestamp, sessionID, part/error}` |
| 本文の在処 | `payload` | `message.content` | `part.text` / `part.state` |
| 識別子 | `payload.id` | `sessionId` / `uuid` / `parentUuid` | `sessionID` / `part.messageID` / `part.callID` |
| `type` 例 | `session_meta` / `response_item` / `event_msg` | `user` / `assistant` / `summary` | `step_start` / `text` / `reasoning` / `tool_use` / `step_finish` / `error` |
| 特徴 | terminalとapiの二重記録 | 会話・ツールが同一系列 | stdoutイベントのみ。prompt/modelは含まれない |

- Codexは `payload`、Claude Codeは `sessionId`、OpenCodeは `sessionID` と `part/error` の組合せを強い判定材料にする。
- 自動判別は `src/detect.rs`(先頭50行をサンプリング)。内容で決まらないときのみ入力パス
  (`rollout-*` / `~/.codex/` / `~/.claude/` / `opencode-*`)を補助シグナルに使う。
- 分岐実装は `src/classify.rs`(正規化)・`src/clean.rs`(keep set)が形式ごとに分かれる。
  `--format auto|codex|claude_code|opencode` で明示上書きも可。
- OpenCode本体の通常保存先は `~/.local/share/opencode/opencode.db`。対応JSONLは
  `opencode run --format json ... > opencode-session.jsonl` で明示保存する。`opencode export` は別形式。
