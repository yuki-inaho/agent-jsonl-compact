# 入力形式と参照資料

**日本語** | [English](INPUT_FORMATS.en.md)

このCLIは3形式を扱いますが、公開された安定スキーマとして扱える範囲は同一ではありません。
各パーサーは未知フィールドを無視し、`--format` による明示指定を残して互換性リスクを局所化します。

| 形式 | 参照URL | このCLIでの位置付け |
|---|---|---|
| Codex CLI 0.152.0 | [公式 `recorder.rs`](https://github.com/openai/codex/blob/rust-v0.152.0/codex-rs/rollout/src/recorder.rs) / [公式互換テスト](https://github.com/openai/codex/blob/rust-v0.152.0/codex-rs/rollout/src/tests.rs) | ローカル導入版 `codex-cli 0.152.0` と同じタグに固定。writerがJSONLを1行ずつ書くこと、`timestamp` / `ordinal` / `type` / `payload` の実例を根拠にする実装由来形式。 |
| Claude Code | [公式 Sessions 文書](https://code.claude.com/docs/en/sessions#where-transcripts-are-stored) / [公式 `.claude` directory 文書](https://code.claude.com/docs/en/claude-directory) | JSONLの保存場所と「各行がmessage/tool/metadata object」であることは公式文書に基づく。一方、同文書は行内スキーマを内部形式・リリースごとに変更可能と明記するため、`sessionId` / `uuid` / `message.content` は対応確認済みのbest-effortとして扱う。 |
| OpenCode 1.18.26 | [CLI公式文書 — `run --format json`](https://dev.opencode.ai/docs/cli/#run) / [v1.18.26 `run.ts`](https://github.com/anomalyco/opencode/blob/v1.18.26/packages/opencode/src/cli/cmd/run.ts) / [SDK型定義](https://github.com/anomalyco/opencode/blob/v1.18.26/packages/sdk/js/src/v2/gen/types.gen.ts) / [session schema](https://github.com/anomalyco/opencode/blob/v1.18.26/packages/schema/src/v1/session.ts) | `run --format json` のstdout NDJSONを対象とし、型・実装を検証対象バージョンへ固定。 |

Codexは実装タグを更新したとき、Claude CodeはCLI更新後、OpenCodeは対応対象バージョンを更新したときに、
合成fixtureを追加して `just check` を通すことを変更の受入条件にします。
