# 作業書: Session JSONL 軽量化スクリプトの Rust バイナリ移植

作成日: 2026-06-09  
対象リポジトリ: `tomato-slack-notion-knowledge-rag-main`  
提出実装: `agent-jsonl-compact` Rust CLI

## 1. 目的

提供リポジトリ内に含まれる Codex CLI rollout JSONL および Claude Code transcript JSONL の軽量化スクリプトを、Python 実行環境に依存しない Rust CLI として切り出し、release build 後に単体バイナリとして配置できる形にします。

本移植で維持する目的は次の 3 点です。

1. 生の巨大 JSONL から、会話・途中過程・ツール実行結果を抽出すること。
2. `clean.jsonl`、`transcript.md`、`summary.json` の 3 種類の成果物を生成すること。
3. 既存スクリプトの「既定は忠実性優先、サイズ削減は明示オプション」という設計を維持すること。

## 2. 移植対象の特定

移植対象は次の Python スクリプトです。

```text
skills/session-transcript-extractor/scripts/extract_session_jsonl.py
```

関連する説明文書は次です。仕様確認・移植根拠として参照しますが、直接の移植対象ではありません。

```text
skills/session-transcript-extractor/SKILL.md
docs/session_transcript_jsonl.md
justfile の extract-session / inspect-session タスク
```

### 2.1 移植する処理単位

既存スクリプトは stage0 から stage4 までの単一パイプラインです。Rust では次の単位へ分割して実装しました。

| Python 側の責務 | 主な関数・処理 | Rust 側の移植先 |
|---|---|---|
| JSONL streaming load | `iter_records` | `src/util.rs` |
| 形式自動判定 | `detect_format` | `src/detect.rs` |
| Codex レコード正規化 | `classify_codex` | `src/classify.rs` |
| Claude Code レコード正規化 | `classify_claude` | `src/classify.rs` |
| keep set / truncate / dedup / elide | `keep_set`, `Cleaner.accept` | `src/clean.rs` |
| Markdown 生成 | `render_markdown`, `fence` | `src/render.rs` |
| CLI、集計、ファイル出力 | `main` | `src/cli.rs`, `src/runner.rs`, `src/main.rs` |

### 2.2 移植しないもの

次は Rust CLI の責務外とします。

- `uv run python ...` 前提の実行形態。
- `skills/session-transcript-extractor/SKILL.md` 自体。
- 既存リポジトリの `justfile` 更新。提出物は独立クレートとして切り出しているため、利用側で必要に応じて `just` タスクを追加します。
- 実セッション由来の JSONL や抽出成果物。PII を含む可能性があるため、実データは提出 ZIP に含めません。

## 3. 採用する実装形態

Rust の独立 binary crate として実装します。

```text
agent-jsonl-compact-rs/
  Cargo.toml
  README.md
  docs/workdoc_agent_jsonl_compactor_rust_port.md
  scripts/build-release.sh
  src/
    main.rs
    lib.rs
    cli.rs
    detect.rs
    classify.rs
    clean.rs
    render.rs
    runner.rs
    util.rs
    counter.rs
  tests/
    fixtures/
      codex_sample.jsonl
      claude_sample.jsonl
    integration.rs
```

この形にする理由は次のとおりです。

- `cargo build --release` で実行ファイルを生成でき、Python や uv を不要にできる。
- `serde_json::Value` ベースで動的 JSON を扱うことで、Codex / Claude Code のスキーマ差分や将来のフィールド追加に強くできる。
- JSONL は streaming read とし、生ログ全体をメモリに載せない。Markdown と summary 生成のため、採用後の compact event のみ `Vec<Event>` として保持する。
- Rust 側で責務を module 分割し、将来の形式追加や option 追加に対応しやすくする。

## 4. CLI 仕様

既存 Python スクリプトの CLI を概ね維持します。

```bash
agent-jsonl-compact -i <session.jsonl> -o <out_dir>
agent-jsonl-compact -i <session.jsonl> --stats
```

主な option は次です。

| option | 意味 | 既定 |
|---|---|---|
| `-i, --input` | 入力 JSONL | 必須 |
| `-o, --out-dir` | 出力ディレクトリ | カレントディレクトリ |
| `--name` | 出力 basename | 入力 stem |
| `--format auto|codex|claude_code` | 形式指定 | `auto` |
| `--channel terminal|api|both` | Codex 用 channel | `terminal` |
| `--msg-chars N` | 会話本文 truncate | `0`、全文保持 |
| `--out-chars N` | tool output truncate | `0`、全文保持 |
| `--elide-outputs` | 肥大 tool output の本文を畳む | false |
| `--keep-token-count` | Codex token_count を残す | false |
| `--no-dedup` | 重複畳み込みを無効化 | false |
| `--format-out jsonl|md|both` | clean / md の出力制御 | `both` |
| `--stats` | 集計表示のみ | false |

出力は次です。

```text
<name>.clean.jsonl
<name>.transcript.md
<name>.summary.json
```

`--format-out jsonl` または `--format-out md` を指定した場合でも、`summary.json` は常に出力します。

## 5. 主要な実装方針

### 5.1 入力形式の自動判定

先頭 50 件の non-empty JSONL レコードを読み、次の条件で判定します。

- Codex: `payload` を持ち、`type` が `session_meta`、`response_item`、`event_msg`、`turn_context`、`compacted` のいずれか。
- Claude Code: `payload` を持たず、`sessionId`、`uuid`、`parentUuid` のいずれかを持ち、`type` が Claude Code 系 type。

判定の優先順は既存 Python と同等です。Claude hit が Codex hit を上回れば `claude_code`、Codex hit があれば `codex`、どちらもなければ `codex` を既定とします。

### 5.2 正規化 event schema

両形式を次のような共通 event kind に正規化します。

```text
session
user
assistant
thinking
reasoning
tool_call
tool_output
patch
web_search
mcp_tool
goal
turn_start
turn_end
turn_aborted
compacted
context_compacted
thread_rolled_back
item_completed
parse_error
```

Codex の API channel 用には次も保持可能です。

```text
api_user
api_assistant
api_developer
```

Claude Code の TUI / 状態メタデータは `cc_meta` として分類しますが、既定 keep set では出力しません。

### 5.3 軽量化ポリシー

既定はシグナル保持を優先します。

- 会話本文と tool output は `--msg-chars 0`、`--out-chars 0` により全文保持。
- Codex `token_count` は telemetry とみなし既定で除外。
- Claude Code の `ai-title`、`mode`、`attachment` 等は既定で除外。
- 連続重複本文と重複 web search は既定で畳み込み。
- サイズ優先が必要な場合のみ、`--msg-chars`、`--out-chars`、`--elide-outputs` を明示的に使用。

### 5.4 バイナリ化

通常の release build は次です。

```bash
cargo test
cargo build --release
```

生成物は次です。

```text
target/release/agent-jsonl-compact
```

配置例です。

```bash
install -m 0755 target/release/agent-jsonl-compact ~/.local/bin/agent-jsonl-compact
```

Linux でより単体配布に近づける場合は、musl target を使います。

```bash
rustup target add x86_64-unknown-linux-musl
cargo build --release --target x86_64-unknown-linux-musl
```

## 6. テスト方針

提出クレートには合成 fixture を含めています。実セッション由来のデータは含めません。

- `tests/fixtures/codex_sample.jsonl`: Codex rollout の最小サンプル。
- `tests/fixtures/claude_sample.jsonl`: Claude Code transcript の最小サンプル。
- `tests/integration.rs`: 自動判定、出力生成、telemetry/meta 除外を確認。
- `src/clean.rs` unit test: truncate と dedup の確認。
- `src/render.rs` unit test: Markdown fence の破損防止確認。

実行コマンドは次です。

```bash
cargo test
```

## 7. DOD

| DOD | 判定 | 内容 |
|---|---:|---|
| 移植対象の所在を特定している | 達成 | `skills/session-transcript-extractor/scripts/extract_session_jsonl.py` を移植対象として特定。 |
| 移植対象の処理単位を整理している | 達成 | stage0 から stage4 を Rust module へ対応付け。 |
| Rust binary crate として独立実装している | 達成 | `agent-jsonl-compact` として `Cargo.toml`、`src/main.rs`、各 module を作成。 |
| 既存 CLI option の主要機能を維持している | 達成 | `--format`、`--channel`、`--msg-chars`、`--out-chars`、`--elide-outputs`、`--stats` 等を実装。 |
| Codex / Claude Code の自動判定を実装している | 達成 | `src/detect.rs` に実装。 |
| Codex / Claude Code の正規化 event 抽出を実装している | 達成 | `src/classify.rs` に実装。 |
| clean JSONL / transcript MD / summary JSON を出力する | 達成 | `src/runner.rs`、`src/render.rs` に実装。 |
| 単体バイナリ化手順を文書化している | 達成 | README と本作業書に build / install 手順を記載。 |
| テスト fixture と統合テストを同梱している | 達成 | `tests/fixtures` と `tests/integration.rs` を作成。 |
| 実データを提出物に含めていない | 達成 | 合成 fixture のみ同梱。 |
| ZIP として提出できる | 達成 | `agent-jsonl-compact-rs.zip` として梱包。 |

### 検証上の制約

本作業環境には `rustc` / `cargo` が存在しないため、この環境内での `cargo test` と `cargo build --release` は実行できませんでした。代替として、実行可能な範囲でファイル構成、Rust ソースの括弧対応、README / workdoc / fixture / test の同梱を確認しています。利用環境での最終検証コマンドは次です。

```bash
cargo test
cargo build --release
```

## 8. 提出物

```text
agent-jsonl-compact-rs.zip
```

ZIP には Rust 実装、README、作業書、テスト fixture、統合テストを含めています。

## 9. 現リポジトリ統合メモ（2026-06-10）

初期提出時は ZIP 前提の作業書だったが、現在は独立 Git リポジトリ
`/home/inaho-omen/Project/agent-jsonl-compact` として運用する。

現行の正本は次のとおり。

| 項目 | 現行値 |
|---|---|
| GitHub | `git@github.com:yuki-inaho/agent-jsonl-compact.git` |
| crate name | `agent-jsonl-compact` |
| CLI binary | `target/release/agent-jsonl-compact` |
| install path example | `~/.local/bin/agent-jsonl-compact` |
| integration user | `/home/inaho-omen/Project/slack-knowledge-rag` の `just extract-session` / `just inspect-session` |

`justfile`、README、Cargo manifest、CLI help、統合先リポジトリの `.envrc` は
`agent-jsonl-compact` 名で統一する。旧名 `session-jsonl-compact` は使用しない。

現在の開発環境では Rust/Cargo が利用可能であり、最終検証は次で行う。

```bash
just check
just demo
cargo build --release
```

`demo-out/` と `target/` は生成物であり、Git 管理対象にしない。

## 10. 配布とスキル統合(2026-06-11)

`agent-jsonl-compact` を「ビルド済みバイナリ + エージェントスキル」として配布可能にする作業を
追加した。Rust 環境を持たない利用者(および他マシン)が導入できる状態を目的とする。

### 10.1 追加した成果物

| 区分 | 成果物 | 役割 |
|---|---|---|
| 配布 CI | `.github/workflows/release.yml` | `v*` タグで musl 静的バイナリを tarball+sha256 にし Releases へ添付 |
| 品質 CI | `.github/workflows/ci.yml` | push/PR で fmt-check + clippy(-D warnings) + test |
| インストーラ | `install.sh` | `curl\|bash` で OS/arch 判定→DL→sha256 検証→`~/.local/bin` へ配置 |
| スキル | `skills/agent-jsonl-compact-reader/SKILL.md` | summary→必要箇所だけ段階読みするフロー(Codex/Claude/OpenCode対応) |
| サブコマンド | `agent-jsonl-compact install-skills` | SKILL.md をバイナリ埋め込みし各エージェント skills へ配置(playwright-cli 流) |
| 補助 | `--version` / release `strip` / justfile(`build-musl`/`dist`/`install-release`/`install-skills`) | 配布補助 |

### 10.2 配布方針の決定

- 対象は Linux x86_64 musl 静的バイナリのみ(glibc 非依存・単一ファイル)。
- スキルは本リポジトリ `skills/` に同梱し、Claude Code と Codex の両方から使う。
- `install-skills` は Microsoft `playwright-cli install --skills` と同方式
  (CLI 自身がスキルを各エージェントへ配置)を採用。
- 配布のため GitHub リポジトリを public へ変更(同梱は合成 fixture のみ・PII なし)。

### 10.3 検証(2026-06-11 実施)

| 検証 | 結果 |
|---|---|
| `just check`(fmt/clippy/test、unit3 + integration5) | green |
| musl 静的ビルド(`file` で static-pie / stripped) | OK |
| GitHub Actions `ci` / `release`(v0.1.0) | いずれも success |
| `install.sh` e2e(固定版 / latest / raw URL の3経路) | DL→sha256→配置→`--version` すべて OK |
| `install-skills`(両方 / `--codex-only` / 入力なしエラー) | 期待どおり |

初版 release は `v0.1.0`。`dist/` は `just dist` のローカル生成物であり Git 管理対象にしない。

## 11. OpenCode run JSONL 対応（2026-09-02）

### 11.1 調査結果と入力境界

作業環境の OpenCode 1.18.26 で `opencode debug paths` とCLI helpを確認した。

| 項目 | 確認結果 |
|---|---|
| 通常の永続保存 | `~/.local/share/opencode/opencode.db` (SQLite)。JSONLは自動生成されない |
| 対応入力 | `opencode run --format json ...` のstdoutを保存したNDJSON |
| 非対応入力 | `opencode export` が出す単一JSON文書、SQLite DBそのもの |
| イベント | `step_start`, `text`, `reasoning`, `tool_use`, `step_finish`, `error` |
| 外形 | `{type, timestamp, sessionID, part}` またはerror時の `{..., error}` |

実装根拠は OpenCode v1.18.26 の
[`packages/opencode/src/cli/cmd/run.ts`](https://github.com/anomalyco/opencode/blob/v1.18.26/packages/opencode/src/cli/cmd/run.ts)。
同実装は各イベントを1行ずつ `JSON.stringify(...) + EOL` でstdoutへ出す。OpenCode run JSONLは
応答側の完了イベントストリームであり、ユーザープロンプト、モデル名、cwdは含まれない。
抽出器は欠落情報を推測せず、summaryのmodelsは空配列、sessionのcwd/versionはnullとする。

推奨取得例:

```bash
opencode run --format json "<prompt>" > opencode-session.jsonl
agent-jsonl-compact -i opencode-session.jsonl -o temp/session_extracts
```

ストリームが中断した場合や `step_finish` が無い場合でも、到着済み行だけを抽出する。
完全な会話履歴やユーザープロンプトが必要な用途では、run JSONLだけでは情報不足である。

### 11.2 正規化規約

| OpenCode type | 正規化event | 保持内容 |
|---|---|---|
| `step_start` | `turn_start` | `messageID`, part ID |
| `text` | `assistant` | `part.text` |
| `reasoning` | `reasoning` | `part.text` |
| `tool_use` | `tool_call` + `tool_output` | tool, callID, input, output/error, status |
| `step_finish` | `turn_end` | reason, cost, tokens, messageID, part ID |
| `error` | `error` | 表示用messageと元error object |

`tool_use` はOpenCodeのstdoutではcompleted/error時に1イベントへ入力と結果が同居する。
既存の共通event契約に合わせ、同じcall IDを持つcall/outputの2イベントへ分ける。
`step_finish.tokens` と `cost` はusage情報なので削除せず `turn_end` に保持する。

### 11.3 実装と回帰テスト

- `SessionFormat::OpenCode` と `--format opencode` を追加。
- `sessionID` + OpenCode type + `part/error` を使う内容ベース自動判定を追加。
- `tests/fixtures/opencode_sample.jsonl` は公式スキーマに沿う合成データのみ。実セッション由来情報なし。
- `tests/integration.rs::extracts_opencode_run_sample` で自動判定、session合成、tool分離、
  tokens/cost、Markdown表示を検証。
- Unix millisecond timestampをMarkdownのUTC `HH:MM:SS` として表示。
- README、オンボーディング、readerスキル、`just demo` を3形式へ同期。

検証結果:

```text
cargo fmt --check                         success
cargo clippy --all-targets -- -D warnings success
cargo test                                12 passed (unit 6 + integration 6)
OpenCode fixture --stats                  detected format: opencode
```

### 11.4 参照仕様URL

実装時に参照した一次資料を、何の根拠として使用したかとともに固定する。

| 資料 | URL | 採用した根拠 |
|---|---|---|
| OpenCode CLI公式文書 | <https://dev.opencode.ai/docs/cli/#run> | `run --format json` がraw JSON eventsを出すこと。`export`は単一JSON、`db path`はDB位置表示であること |
| v1.18.26 `run.ts` | <https://github.com/anomalyco/opencode/blob/v1.18.26/packages/opencode/src/cli/cmd/run.ts> | `{type,timestamp,sessionID,...}` を1行ずつ書く処理と、`step_start/text/reasoning/tool_use/step_finish/error` の選択 |
| v1.18.26 SDK v2型定義 | <https://github.com/anomalyco/opencode/blob/v1.18.26/packages/sdk/js/src/v2/gen/types.gen.ts> | `TextPart`, `ReasoningPart`, `ToolState*`, `ToolPart`, `StepStartPart`, `StepFinishPart` のフィールド型 |
| v1.18.26 session schema | <https://github.com/anomalyco/opencode/blob/v1.18.26/packages/schema/src/v1/session.ts> | part discriminator、tool state、token/cache/cost構造のスキーマ側定義 |

可変の公式文書だけでなく、検証対象と同じ `v1.18.26` タグのソースを併記した。
これにより、将来のOpenCode更新で形式が変わった場合に、どのバージョンとの差分を確認すべきか追跡できる。

## 12. Codex / Claude Code の参照仕様と小規模リファクタ（2026-09-02）

### 12.1 形式ごとの根拠とサポート境界

OpenCodeだけでなく、既存のCodex / Claude Code入力についても一次情報を確認した。
形式が同じ安定度で公開されているとは仮定しない。

| 形式 | 資料 | URL | 採用した根拠・境界 |
|---|---|---|---|
| Codex CLI | v0.152.0 rollout writer | <https://github.com/openai/codex/blob/rust-v0.152.0/codex-rs/rollout/src/recorder.rs> | ローカル導入済み `codex-cli 0.152.0` と同じタグ。`RolloutLineRef` が `timestamp` / optional `ordinal` / flattenされた `RolloutItem` をJSONへserializeし、1行ずつ追記すること。 |
| Codex CLI | v0.152.0 rollout tests | <https://github.com/openai/codex/blob/rust-v0.152.0/codex-rs/rollout/src/tests.rs> | `session_meta`、`event_msg`、`response_item` を含むレコードのJSON互換性と、`~/.codex/sessions/YYYY/MM/DD/rollout-*.jsonl` の保存例。 |
| Claude Code | Sessions公式文書 | <https://code.claude.com/docs/en/sessions#where-transcripts-are-stored> | 保存場所 `~/.claude/projects/<project>/<session-id>.jsonl` と、各行がmessage/tool/metadataのJSON objectであること。**行内形式はinternalでリリース間に変更可能**とも明記されている。 |
| Claude Code | `.claude` directory公式文書 | <https://code.claude.com/docs/en/claude-directory> | `~/.claude` がtranscript、prompt history、file snapshots、cache、logを含むアプリケーションデータであり、実ログをPIIとして扱う理由。 |

Codexは公開実装に基づくバージョン固定の互換実装とする。Claude Codeは公開安定スキーマではないため、
`sessionId` / `uuid` / `parentUuid` と既知 `type` の組合せだけで検出し、未知レコードは捨てずにmetaとして扱う。
いずれも、実ログをfixtureへコピーせず、変更時に合成fixtureを追加して確認する。

### 12.2 品質調査と採用した変更

[Mozilla rust-code-analysis](https://github.com/mozilla/rust-code-analysis) のCLI
`rust-code-analysis-cli` 0.0.25を開発環境へ `cargo install rust-code-analysis-cli --locked` で導入した。
これはプロジェクトの実行時・開発時依存には加えない外部解析ツールである。

実測コマンド:

```bash
rust-code-analysis-cli --metrics --output-format json --paths src
cargo fmt --check
cargo clippy --all-targets -- -D warnings
cargo test
```

メトリクス上、`classify_codex_event_msg` (cyclomatic 15, cognitive 1) と
`classify_codex_response_item` (cyclomatic 12, cognitive 6) が目立つ。ただし前者は外部JSONの
discriminatorを平坦な `match` で列挙した結果であり、数値だけを下げるための分割は可読性を悪化させる。
SOLID / KISS / DRY / YAGNIに従い、変更対象は実際に重複していた箇所だけに限定した。

- `detect.rs`: 形式別type一覧を不変定数へ移し、検出のたびに3個の `HashSet` を構築する不要な処理を除去した。
- `classify.rs`: reader由来の `_parse_error` を3形式で同一eventへ正規化していた処理を、1個の小さなヘルパーへ集約した。
- `runner.rs`: Claude Code / OpenCodeの初回session event合成を1関数へ集約し、ループ側は「最初の合成eventを一度だけ採用する」責務に絞った。
- 回帰テスト: 3形式すべてでparse errorのevent契約が同一であることを追加で検証した。

新しいtrait、汎用parser framework、形式横断の複雑な抽象化、CI必須の外部解析器は追加しない。
これは3形式の実データ境界が異なるためであり、拡張点を作るより既存の形式別classifierを明示的に保つ方が安全である。

検証結果:

```text
rust-code-analysis-cli 0.0.25             installed
cargo fmt --check                         success
cargo clippy --all-targets -- -D warnings success
cargo test                                13 passed (unit 7 + integration 6)
```
