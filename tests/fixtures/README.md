# Test fixtures / テスト fixture

## Synthetic fixtures

`codex_sample.jsonl`, `claude_sample.jsonl`, and `opencode_sample.jsonl` are fully
synthetic and contain no real session data.

## `claude_desktop_sample.jsonl`

This fixture is derived from a real Claude Code / Claude Desktop transcript and is
published with permission after sanitization. It keeps the conversation shape needed for
end-to-end tests:

- user / assistant text and thinking blocks
- `Bash` tool calls and tool results
- a final pending tool call (no matching result) for `pending_result` coverage
- Japanese / Unicode text
- a single Claude Code session envelope

Removed before publication:

- system prompt snapshots and tool schemas
- memory context and environment snapshots
- skill / agent / MCP listings
- credential / organization metadata
- UI-only records (`queue-operation`, `last-prompt`, `mode`, `atis-latch`)
- all `attachment` records

Replaced with deterministic placeholders:

- personal names, email addresses
- home / project / temporary paths
- session, message, request, organization, and tool identifiers
- timestamps and timezone (all timestamps start at `2026-01-01T00:00:00.000Z`)
- URLs, IP addresses, and credential-looking strings

The fixture is intentionally small (11 records). If you find residual sensitive data,
report it privately to the maintainer instead of opening a public issue.

---

## 合成 fixture

`codex_sample.jsonl` / `claude_sample.jsonl` / `opencode_sample.jsonl` は完全な合成データで、
実セッションを含みません。

## `claude_desktop_sample.jsonl`

実在の Claude Code / Claude Desktop transcript から、許可を得たうえで機密情報を除去して
作成した fixture です。E2E テストに必要な会話構造だけを残しています。

残したもの:

- user / assistant の text と thinking
- `Bash` の tool call / tool result
- 末尾の pending tool call（`pending_result` の確認用）
- 日本語 / Unicode テキスト
- 単一の Claude Code session envelope

公開前に削除したもの:

- system prompt snapshot と tool schema
- memory context / environment snapshot
- skill / agent / MCP listing
- credential / organization metadata
- UI 専用 record（`queue-operation` / `last-prompt` / `mode` / `atis-latch`）
- すべての `attachment` record

決定的なプレースホルダへ置換したもの:

- 個人名、メールアドレス
- home / project / temporary path
- session / message / request / organization / tool identifier
- timestamp と timezone（すべて `2026-01-01T00:00:00.000Z` 開始）
- URL、IP address、credential らしき文字列

fixture は意図的に小さくしています（11 records）。残存する機密情報を見つけた場合は、
public issue ではなく maintainer へ非公開で連絡してください。
