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
