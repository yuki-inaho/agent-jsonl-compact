---
name: agent-jsonl-compact-reader
description: >-
  Compact and read large Codex CLI, Claude Code, or OpenCode run JSONL logs
  without loading the raw file into context. Use when asked to read, summarize,
  inspect, or investigate an existing session transcript, especially when the
  raw JSONL is large.
  Triggers: 過去セッションのjsonlを読む/要約する, セッションログを軽量化して読み込む,
  rollout jsonl を読む, transcript jsonl を要約, agent-jsonl-compact で抽出.
---

# agent-jsonl-compact-reader

巨大な Codex / Claude Code / OpenCode run セッション JSONL を、生のままコンテキストへ載せず
`agent-jsonl-compact` バイナリで軽量化し、`summary.json` → 必要箇所だけ
`transcript.md` / `clean.jsonl` の順に**段階的に読む**ためのスキル。

## When to use

- 「この過去セッションの jsonl を読んで / 要約して」
- `~/.codex/sessions/YYYY/MM/DD/rollout-*.jsonl` や
  `~/.claude/projects/<proj>/<uuid>.jsonl` の内容把握・調査
- `opencode run --format json` を保存したNDJSONの内容把握・調査
- 生 JSONL が大きく、全文を読むとコンテキストを圧迫する場合

入力の典型的な所在:

```text
Codex       ~/.codex/sessions/<YYYY>/<MM>/<DD>/rollout-*.jsonl
Claude Code ~/.claude/projects/<project-slug>/<uuid>.jsonl
OpenCode    保存先は任意(opencode run --format json > opencode-session.jsonl)
```

OpenCode 1.18系は通常 `~/.local/share/opencode/opencode.db` に永続化し、JSONLを自動保存しない。
`opencode export` の単一JSON文書も対象外。対応入力が必要ならstdoutを明示的に保存する。
OpenCode run JSONLにはユーザープロンプトとモデル名が含まれないため、抽出結果にも現れない。

## Step 0 — ensure the binary

```bash
command -v agent-jsonl-compact && agent-jsonl-compact --version
```

無ければ導入（どちらか）:

```bash
# prebuilt (Linux x86_64 musl)
curl -fsSL https://raw.githubusercontent.com/yuki-inaho/agent-jsonl-compact/main/install.sh | bash

# またはソースから(リポジトリ内で)
just install        # ~/.local/bin/agent-jsonl-compact
```

このスキル自体はバイナリから各エージェントへ配置できる:

```bash
agent-jsonl-compact install-skills                 # ~/.claude/skills と ~/.codex/skills(存在する側)
agent-jsonl-compact install-skills --claude-only   # または --codex-only
```

## Step 1 — (任意) 形式とレコード分布だけ確認

抽出せず load→detect→classify の結果だけ見たいとき:

```bash
agent-jsonl-compact -i <input.jsonl> --stats
```

## Step 2 — 3 成果物を生成

既定は忠実モード（`--msg-chars 0`/`--out-chars 0` 相当 = 全文保持、純ノイズのみ除去）。
**出力先は PII を避けて `temp/` 配下など .gitignore 済みの場所**にする。

```bash
agent-jsonl-compact -i <input.jsonl> -o temp/session_extracts
```

生成物（`<name>` は入力 stem、または `--name`。`--format-out jsonl|md|both` で
clean.jsonl / transcript.md の片方だけにできる。summary.json は常に出る）:

```text
<name>.summary.json     形式 / 件数 / models / goals / 入力比などのメタ
<name>.transcript.md    人間可読の会話・思考・ツール実行
<name>.clean.jsonl      正規化 event の構造化 JSONL(grep 向き)
```

## Step 3 — まず summary.json を読む

`format` / `kept_events` / `models` / `goals` / `input_bytes` と出力比で
全体規模と中身の当たりを付ける。**ここでコンテキスト消費を最小化する。**

## Step 4 — 目的で読み分ける

- 全体把握・人間可読 → `<name>.transcript.md` を読む
- 特定調査（コマンド・エラー・ファイル名で探す）→ `clean.jsonl` を grep して
  ヒット周辺の行だけ読む:

  ```bash
  grep -n "keyword" temp/session_extracts/<name>.clean.jsonl
  ```

## Step 5 — それでも大きすぎる時だけ lossy ノブ

```bash
agent-jsonl-compact -i <input.jsonl> -o temp/session_extracts \
  --msg-chars 4000 --out-chars 2000 --elide-outputs
```

- `--elide-outputs` 肥大ツール出力を件数+先頭行へ畳む
- `--channel api`(Codex のみ) API 本文中心に絞る（`both` は terminal と api の両方）
- 形式が誤判定される場合のみ `--format codex|claude_code|opencode`
- 調査用に残したい場合だけ `--keep-token-count`（Codex token_count）/ `--no-dedup`（重複の畳み込み無効）

## Codex の新しい rollout で本文が空になる場合

`summary.json` の `kept_kind_counts` が `item_completed` と `reasoning` ばかりで、
`transcript.md` に `📦 item_completed: AgentMessage` のような見出ししか無いときは、
既定の `--channel terminal` が新しい Codex CLI のイベント本文を拾えていない。
`--channel api`（または `both`）で抽出し直すと、ユーザー発言・アシスタント発言・
`function_call` が本文付きで残る。出力名が衝突しないよう別ディレクトリへ出す:

```bash
agent-jsonl-compact -i <rollout.jsonl> -o temp/session_extracts_api --channel api
```

コードモードのツール実行（`response_item` の `custom_tool_call` / `custom_tool_call_output`）や
エージェント間メッセージはどのチャネルでも抽出されない。`summary.json` の
`raw_type_counts` に件数が出ていて中身が必要なときだけ、raw JSONL から該当行を直接抜く:

```bash
python3 - <rollout.jsonl> <<'PY'
import json, sys
for line in open(sys.argv[1]):
    payload = json.loads(line).get("payload", {})
    if payload.get("type") in ("custom_tool_call", "custom_tool_call_output"):
        print(json.dumps(payload, ensure_ascii=False)[:2000])
PY
```

reasoning は暗号化されている（`[encrypted]`）ため、どの方法でも復元できない。

## Notes

- 既定はシグナル保持優先。サイズ削減は上記ノブを**明示したときだけ**効く。
- 成果物は会話本文・ホームパス等 **PII を含みうる。コミットしない。**
- 1 入力につき再実行は冪等（同じ出力名を上書き）。
