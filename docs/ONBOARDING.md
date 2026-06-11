# LLMオンボーディングサマリー

> このテンプレートは、新任LLMエージェントがプロジェクトに参加する際に共有する初期資料のたたき台です。各項目を埋め、参照元ドキュメントや補足リンクがあれば併記してください。

## 1. プロジェクト概要と目的
- **プロジェクト名称・領域:** `agent-jsonl-compact` — Codex CLI / Claude Code のセッション JSONL を軽量化する単一 Rust CLI と、それを使うエージェントスキル。
- **最終成果物:** 生 JSONL を自動判定し `*.clean.jsonl` / `*.transcript.md` / `*.summary.json` を生成する単一バイナリ。加えて reader スキル(`agent-jsonl-compact-reader`)と musl 配布(`install.sh` / GitHub Releases)。
- **ビジネス背景・価値:** 巨大なセッションログを生のままコンテキストへ載せず「軽量化 → 段階読み」でトークンを節約する。`slack-knowledge-rag` 等の周辺ツールから PATH 上の CLI として利用される(`just extract-session` / `inspect-session`)。
- **現時点の進捗サマリ:** v0.1.0 リリース済み。CI(`ci`/`release`)・配布(musl/`install.sh`)・スキル(`install-skills`)整備完了(2026-06-11)。`確認済み`: ローカル `just check` green、GitHub Actions success、`install.sh` の e2e(固定/latest/raw) OK。

## 2. クリティカルな要求・制約
> 「壊してはいけない」品質・仕様ラインを箇条書きで列挙します。
- 既定は**忠実モード**。シグナル(全メッセージ本文・全ツール出力・全ツール呼出)は削らない。lossy(`--msg-chars`/`--out-chars`/`--elide-outputs`/`--channel api`)は明示指定時のみ作用させる。
- 実セッション JSONL と抽出物(`*.clean.jsonl`/`*.transcript.md`/`*.summary.json`)は会話本文・ホームパス等の **PII を含む。コミット禁止**(`.gitignore` 済)。同梱 fixture は合成データのみ。
- JSONL は **streaming load**。生ログ全体をメモリへ載せない(`src/util.rs`)。保持するのは採用後 event のみ。
- 形式自動判定の優先順(`src/detect.rs`)と正規化 event kind / keep set(`src/classify.rs`・`src/clean.rs`)を破壊しない。
- 配布対象は **Linux x86_64 musl 静的のみ**。`install.sh` は他 OS/arch を明示エラーにする。
- `SKILL.md` はビルド時に `include_str!` でバイナリへ埋め込み。**スキル本文を変更したら再ビルドが必要**。
- リリースは `v*` タグ → `release.yml`。リポジトリは public。

## 3. 参照すべき合意済み資料
> 新任エージェントが必ず確認すべき一次資料の一覧です。パスと役割を記載します。
| 種別 | ファイル/リンク | 概要・用途 |
|------|------------------|------------|
| 概要・使い方 | `README.md` | インストール / 使い方 / オプション / S/N 方針 / 開発レイアウト |
| 設計・作業記録 | `docs/workdoc_session_jsonl_compactor_rust_port.md` | 移植根拠・モジュール対応・配布/スキル統合(章10)・DOD |
| スキル仕様 | `skills/agent-jsonl-compact-reader/SKILL.md` | reader スキルの段階読みフロー(summary → 必要箇所だけ) |
| WBS / 進捗 | `docs/workdoc_...md` 章9–10 | 統合メモと配布・スキル作業の記録 |
| タスク定義 | `justfile` | build / test / check / demo / dist / install* |
| テスト資産 | `tests/integration.rs`, `tests/fixtures/*.jsonl` | 自動判定・出力・install-skills の確認(合成 fixture) |
| 既知課題リスト | 未確認 | 専用課題リストは未整備。GitHub Issues は `推定`(未確認) |

## 4. タスク境界（任せること / 任せないこと）
### 任せるタスク（例）
- 新形式 / 新レコード type の対応(`src/detect.rs`・`src/classify.rs`)。
- CLI option・出力レンダリングの追加改善(`src/cli.rs`・`src/render.rs`)。
- CI・配布・`install.sh`・スキルの改善、テスト追加(`tests/`)。
- ドキュメント更新(README / workdoc / SKILL.md)。

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
2. `agent-jsonl-compact -i tests/fixtures/codex_sample.jsonl --stats` で形式とレコード型分布を確認する。
3. 一時 HOME で `HOME=$(mktemp -d ...) agent-jsonl-compact install-skills` を実行し、`.claude`/`.codex` 配下への配置を確認する(実 HOME を汚さない)。

## 7. 運用ルール・変更管理
- **ドキュメント更新時の記載ルール:** 作業は `docs/workdoc_*.md` に章番号を継続して追記(現在 章10 まで)。相対日付は絶対日付に変換する。
- **TBDの扱い:** 未確定は `未確認` と、次に当たるべき一次資料を明記する。
- **レビュー/承認フロー:** CI(`ci.yml`: fmt-check + clippy(-D warnings) + test)を green に保つ。変更は動作検証の出力で裏付ける。リリースは `v*` タグ push で `release.yml` が走る。
- **その他の運用ルール:** 生成物(`target/` / `demo-out/` / `dist/`)は Git 管理外。`install.sh` は sha256 検証付き。

---

### 付録: 参考情報
- **主要リポジトリ/ディレクトリ:** `src/`(CLI 実装), `skills/agent-jsonl-compact-reader/`(スキル), `.github/workflows/`(CI), `tests/`(fixtures + integration), `docs/`(作業書 / 本書), `install.sh`。
- **代表的なコマンド:**
  - `just check` / `just demo` / `just dist`
  - `agent-jsonl-compact -i <session.jsonl> -o <out_dir>`
  - `agent-jsonl-compact --stats` / `agent-jsonl-compact install-skills [--claude-only|--codex-only]`
- **依存ライブラリ:** `anyhow`, `clap`(derive), `serde`, `serde_json`(dev: `tempfile`)。詳細は `Cargo.toml`。
- **連絡先/責任者:** git author `yuki-inaho`(yoshikawa@inaho.co)。GitHub: `yuki-inaho/agent-jsonl-compact`。

> ※テンプレートは必要に応じて拡張・縮退して構いません。記入済みのドキュメントはバージョン管理してください。

---

### 付録: セッション JSONL フォーマット早見

Codex と Claude Code の JSONL は**別スキーマ**。共通点は「1行1 JSON」「`type` と `timestamp` を持つ」程度。

| | Codex CLI rollout | Claude Code transcript |
|---|---|---|
| 1行の形 | `{type, payload, timestamp}` | `{type, message, sessionId, uuid, parentUuid, cwd, ...}` |
| 本文の在処 | `payload` の中(さらに `payload.type` で再分類) | `message.content`(文字列 or ブロック配列) |
| 識別子 | `payload.id` | `sessionId` / `uuid` / `parentUuid`(ツリー構造) |
| `type` 例 | `session_meta` / `response_item` / `event_msg` / `turn_context` / `compacted` | `user` / `assistant` / `summary` / `ai-title` / `mode` |
| 特徴 | terminal(`event_msg`)と api(`response_item`)の**二重記録** | 単一系列、`message.content` にツール呼出/結果が混在 |

- **決定的な見分け方は `payload` キーの有無**(Codex は必ず持つ、Claude Code は持たない)。
- 自動判別は `src/detect.rs`(先頭50行をサンプリング)。内容で決まらないときのみ入力パス
  (`rollout-*` / `~/.codex/` / `~/.claude/`)を補助シグナルに使う。
- 分岐実装は `src/classify.rs`(正規化)・`src/clean.rs`(keep set)が両形式で完全に分かれる。
  `--format auto|codex|claude_code` で明示上書きも可。
