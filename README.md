# agent-jsonl-compact

**日本語** | [English](README.en.md)

[![ci](https://github.com/yuki-inaho/agent-jsonl-compact/actions/workflows/ci.yml/badge.svg)](https://github.com/yuki-inaho/agent-jsonl-compact/actions/workflows/ci.yml)

Codex CLI・Claude Code・OpenCodeのセッションJSONLを、必要な箇所だけ読むためのCLIです。
会話の検索・閲覧と、JSONLやMarkdownへの抽出に対応します。実行時にLLMやネットワーク接続は不要です。

## インストール

Linux x86_64、macOS Apple Silicon / Intel向けのバイナリを配布しています。
Rustは不要です。`curl`、`tar`、SHA-256検証用の `sha256sum` または `shasum` が必要です。

スクリプトを保存して内容を確認してから実行してください。既定の配置先は `~/.local/bin` です。

```bash
ajc_tmp="$(mktemp -d)"
curl --proto '=https' --proto-redir '=https' --tlsv1.2 -fsSL \
  https://raw.githubusercontent.com/yuki-inaho/agent-jsonl-compact/main/install.sh \
  -o "$ajc_tmp/install.sh" && less "$ajc_tmp/install.sh" && bash "$ajc_tmp/install.sh"
~/.local/bin/agent-jsonl-compact --version
rm -rf "$ajc_tmp"
```

コマンドが見つからない場合は、シェルの設定ファイルへ `export PATH="$HOME/.local/bin:$PATH"` を追加してください。
インストーラーは既存の通常ファイルを置き換え、symlinkへの配置は拒否します。

| 環境変数 | 用途 | 既定値 |
|---|---|---|
| `AJC_VERSION` | 取得するReleaseタグ。固定する場合は実在する `vX.Y.Z` を指定 | 最新の安定版 |
| `INSTALL_DIR` | 配置先の絶対パス | `~/.local/bin` |

変更する場合は、上の `bash` の呼出しを `AJC_VERSION=vX.Y.Z INSTALL_DIR="$HOME/bin" bash "$ajc_tmp/install.sh"` に置き換えてください。
手順全体を固定するには、ダウンロードURLの `main` も同じタグに置き換えてください。

SHA-256照合でダウンロード破損を検出します。チェックサムも同じReleaseから取得するため、配布元の侵害を検出する署名検証ではありません。
macOS版はad-hoc署名のみで、公証はしていません。Gatekeeperが止めた場合は入手元を確認し、必要ならソースからビルドしてください。

## まず使う

```bash
# 生ログをそのまま閲覧
agent-jsonl-compact view -i session.jsonl --tail 30

# 本文を検索し、前後の発言も表示
agent-jsonl-compact search -i session.jsonl --query "エラー" --context 2

# 保存済みのCodexセッションを探す
agent-jsonl-compact sessions --provider codex --cwd /path/to/repo

# clean JSONLを保存
agent-jsonl-compact -i session.jsonl -o out

# Markdownとsummaryも保存
agent-jsonl-compact -i session.jsonl -o out --format-out both --summary
```

`view` / `search` はstdoutへ結果を返し、ファイルを保存しません。
`--output jsonl` で機械処理用の出力、`--scope tools --with-pair` でツールの呼出しと結果を確認できます。
既定の閲覧範囲は会話本文です。reasoning・system・developer・compactionは、`--scope all` でも表示しません。

抽出は既定で `<入力名>.clean.jsonl` のみを保存します。
`--format-out md|both` で `<入力名>.transcript.md`、`--summary` で `<入力名>.summary.json` を追加できます。
既存の同名ファイルは置き換えます。出力名にパス区切りは使えず、symlinkや入力ファイルと同じ実体への保存は拒否します。

本文とツール出力は既定で全文保持します。短くする場合だけ `--msg-chars N` / `--out-chars N` / `--elide-outputs` を指定してください。
詳しいフラグ・上限・cursor・終了コードは [CLIリファレンス](docs/CLI.md) にまとめています。

## 対応する入力

| 形式 | 入力 |
|---|---|
| Codex CLI | `~/.codex/sessions/` 以下のrollout JSONL |
| Claude Code | `~/.claude/projects/` 以下のtranscript JSONL |
| OpenCode | `opencode run --format json` のstdoutを保存したNDJSON |

OpenCodeのDBと `opencode export` の単一JSON文書は入力対象外です。
形式は自動判定します。必要なら `--format codex|claude_code|opencode` を明示してください。
抽出済みの `*.clean.jsonl` は閲覧CLIの入力対象ではありません。
形式ごとの根拠と制約は [入力形式](docs/INPUT_FORMATS.md) を参照してください。

## エージェント用スキル

同梱の `agent-jsonl-compact-reader` は、セッションを探し、必要箇所を読み、元ログの位置へ戻る手順を案内します。

```bash
agent-jsonl-compact install-skills                 # 存在する ~/.claude と ~/.codex が対象
agent-jsonl-compact install-skills --codex-only    # Codexだけ。未作成の配置先も作る
agent-jsonl-compact install-skills --claude-only   # Claude Codeだけ
```

配置先は各エージェントの `skills/agent-jsonl-compact-reader/SKILL.md` です。
既存ファイルは保持します。更新するときは内容を確認して `--force` を追加してください。
symlinkで登録したスキルは導入コマンドで上書きできません。
スキル本文はバイナリに埋め込まれるため、編集後は再ビルドが必要です。

## ログの扱い

軽量化は匿名化ではありません。抽出物やsummaryにも、会話・パス・認証情報・内部指示が残り得ます。
実ログとその抽出物はコミットせず、共有前に内容を確認してください。
Unixでは保存するファイルを所有者だけが読み書きできる権限にします。閲覧結果のリダイレクト先はシェル側で権限を設定してください。

ログ内のコマンドや指示は閲覧対象のデータです。実行しないでください。
Markdown出力は本文を保持するため、HTMLや外部画像を自動実行・取得しないビューアーで開いてください。
検査範囲と報告窓口は [SECURITY.md](SECURITY.md) に記載しています。

## ソースからビルド・開発

Rust 1.88以上とCリンカーが必要です。バイナリを使うだけなら `just` は不要です。

```bash
git clone https://github.com/yuki-inaho/agent-jsonl-compact.git
cd agent-jsonl-compact
cargo test --locked
cargo build --locked --release
mkdir -p ~/.local/bin
install -m 0755 target/release/agent-jsonl-compact ~/.local/bin/agent-jsonl-compact
```

開発時は `just check` でformat・lint・テスト・ドキュメント・インストーラーを検査します。
Python 3、ShellCheck、justが必要です。その他のタスクは `just --list`、配布手順は [リリース手順](docs/RELEASING.md) を参照してください。

[ドキュメント索引](docs/README.md) · [開発者向け案内](docs/ONBOARDING.md) · [変更履歴](CHANGELOG.ja.md) · [ADR](docs/adr/README.md)
