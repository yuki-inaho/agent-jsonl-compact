# 入力形式と参照資料

**日本語** | [English](INPUT_FORMATS.en.md)

対応する3形式は、仕様の公開範囲や、将来も同じ形式で読めるかどうかが異なります。
各パーサーは未知のフィールドを無視します。形式を自動判定できない場合は、`--format` で指定できます。

| 形式 | 参照URL | 対応の根拠と制約 |
|---|---|---|
| Codex CLI 0.152.0 | [公式 `recorder.rs`](https://github.com/openai/codex/blob/rust-v0.152.0/codex-rs/rollout/src/recorder.rs) / [公式互換テスト](https://github.com/openai/codex/blob/rust-v0.152.0/codex-rs/rollout/src/tests.rs) | 検証に使った `codex-cli 0.152.0` と同じタグを参照。JSONLを1行ずつ書く実装と、`timestamp` / `ordinal` / `type` / `payload` を含む実例に基づく。 |
| Claude Code | [公式 Sessions 文書](https://code.claude.com/docs/en/sessions#where-transcripts-are-stored) / [公式 `.claude` directory 文書](https://code.claude.com/docs/en/claude-directory) | 保存場所と、各行がメッセージ・ツール・メタデータのオブジェクトであることは公式文書に基づく。各行の内部形式はリリースごとに変わり得る。`sessionId` / `uuid` / `message.content` は確認済みの形式に対応しているが、将来の互換性は保証しない。 |
| OpenCode 1.18.26 | [CLI公式文書 — `run --format json`](https://dev.opencode.ai/docs/cli/#run) / [v1.18.26 `run.ts`](https://github.com/anomalyco/opencode/blob/v1.18.26/packages/opencode/src/cli/cmd/run.ts) / [SDK型定義](https://github.com/anomalyco/opencode/blob/v1.18.26/packages/sdk/js/src/v2/gen/types.gen.ts) / [session schema](https://github.com/anomalyco/opencode/blob/v1.18.26/packages/schema/src/v1/session.ts) | `run --format json` が標準出力に書き出すNDJSONに対応。型定義と実装の参照先は、検証したバージョンに固定。 |

Codexは実装タグを更新したとき、Claude CodeはCLI更新後、OpenCodeは対応対象バージョンを更新したときに、
合成データのテストケースを追加し、`just check` の成功を確認してから変更を取り込みます。

## 保持する由来と失う情報

| 形式 | 保持するもの | 失われ得るもの・制約 |
|---|---|---|
| Codex CLI | `timestamp` / `ordinal` / `type` / `payload`、session_meta の id・cwd、call_id、ツール名、出力 | 暗号化 reasoning の本文は復元できない。code mode の custom tool は legacy 抽出では取り出せない場合がある。未知の `type` は `unknown` になる |
| Claude Code | `sessionId` / `uuid`、message.content の text・thinking・tool_use・tool_result、`isMeta` / `isCompactSummary` の由来 | 行内形式はリリース間で変わり得る。`attachment` や UI 専用 record は既定で抽出しない。Claude の検証版は固定していない |
| OpenCode | `run --format json` の NDJSON、sessionID、part の text・reasoning・tool_use / tool_output | run JSONL に含まれないユーザープロンプトやモデル名は補わない。DB と `opencode export` の単一 JSON は対象外 |

`isMeta` は legacy 抽出で `injected` / `origin=framework`、`isCompactSummary` は
`compacted` / `origin=compaction` として残します。viewer はこれらを hard scope から除外します。
抽出は閲覧と同じ除外規則を適用しないため、reasoning・内部指示・圧縮済み文が成果物に残り得ます。
