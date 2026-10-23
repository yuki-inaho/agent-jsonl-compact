# Input formats and sources

[日本語](INPUT_FORMATS.md) | **English**

The CLI handles three formats, but the publicly documented stable schema surface differs
between them. Each parser ignores unknown fields, and `--format` remains available to pin
the format explicitly, limiting compatibility risk.

| Format | Reference | Position in this CLI |
|---|---|---|
| Codex CLI 0.152.0 | [official `recorder.rs`](https://github.com/openai/codex/blob/rust-v0.152.0/codex-rs/rollout/src/recorder.rs) / [official compatibility tests](https://github.com/openai/codex/blob/rust-v0.152.0/codex-rs/rollout/src/tests.rs) | Pinned to the same tag as the locally installed `codex-cli 0.152.0`. The implementation-derived format is supported by the writer emitting one JSONL line at a time and by real examples of `timestamp` / `ordinal` / `type` / `payload`. |
| Claude Code | [official Sessions docs](https://code.claude.com/docs/en/sessions#where-transcripts-are-stored) / [official `.claude` directory docs](https://code.claude.com/docs/en/claude-directory) | The storage location and the fact that each line is a message/tool/metadata object are based on official docs. The same docs state that the in-line schema is internal and may change between releases, so `sessionId` / `uuid` / `message.content` are handled as verified best-effort fields. |
| OpenCode 1.18.26 | [CLI docs — `run --format json`](https://dev.opencode.ai/docs/cli/#run) / [v1.18.26 `run.ts`](https://github.com/anomalyco/opencode/blob/v1.18.26/packages/opencode/src/cli/cmd/run.ts) / [SDK type definitions](https://github.com/anomalyco/opencode/blob/v1.18.26/packages/sdk/js/src/v2/gen/types.gen.ts) / [session schema](https://github.com/anomalyco/opencode/blob/v1.18.26/packages/schema/src/v1/session.ts) | Targets stdout NDJSON from `run --format json`; types and implementation are pinned to the verified version. |

When the Codex implementation tag is updated, after a Claude Code CLI update, or when the
supported OpenCode version is updated, add synthetic fixtures and pass `just check` as the
acceptance condition for the change.
