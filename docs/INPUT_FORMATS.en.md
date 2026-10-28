# Input formats and sources

[日本語](INPUT_FORMATS.md) | **English**

The three supported formats differ in how much of their structure is documented and whether that structure is guaranteed to remain stable.
Each parser ignores unknown fields. Use `--format` to select the format if automatic detection fails.

| Format | Reference | Basis for support and limitations |
|---|---|---|
| Codex CLI 0.152.0 | [official `recorder.rs`](https://github.com/openai/codex/blob/rust-v0.152.0/codex-rs/rollout/src/recorder.rs) / [official compatibility tests](https://github.com/openai/codex/blob/rust-v0.152.0/codex-rs/rollout/src/tests.rs) | References use the same tag as the tested `codex-cli 0.152.0`. Support is based on the code that writes one JSONL line at a time and examples containing `timestamp` / `ordinal` / `type` / `payload`. |
| Claude Code | [official Sessions docs](https://code.claude.com/docs/en/sessions#where-transcripts-are-stored) / [official `.claude` directory docs](https://code.claude.com/docs/en/claude-directory) | Official docs describe the storage location and each line as a message, tool, or metadata object. The internal line format may change between releases. The parser supports verified forms of `sessionId` / `uuid` / `message.content`; future compatibility is not guaranteed. |
| OpenCode 1.18.26 | [CLI docs — `run --format json`](https://dev.opencode.ai/docs/cli/#run) / [v1.18.26 `run.ts`](https://github.com/anomalyco/opencode/blob/v1.18.26/packages/opencode/src/cli/cmd/run.ts) / [SDK type definitions](https://github.com/anomalyco/opencode/blob/v1.18.26/packages/sdk/js/src/v2/gen/types.gen.ts) / [session schema](https://github.com/anomalyco/opencode/blob/v1.18.26/packages/schema/src/v1/session.ts) | Targets stdout NDJSON from `run --format json`; types and implementation are pinned to the verified version. |

After updating the Codex reference tag, the Claude Code CLI, or the supported OpenCode version,
add synthetic test fixtures and confirm that `just check` passes before accepting the change.

## Provenance kept and information lost

| Format | Kept | Lost or limited |
|---|---|---|
| Codex CLI | `timestamp` / `ordinal` / `type` / `payload`, session_meta id/cwd, call_id, tool name, output | Encrypted reasoning bodies cannot be restored. Code-mode custom tools may be dropped by legacy extraction. Unknown `type` values become `unknown` |
| Claude Code | `sessionId` / `uuid`, text/thinking/tool_use/tool_result in message.content, provenance of `isMeta` / `isCompactSummary` | The internal line format may change between releases. `attachment` and UI-only records are omitted by default. No Claude version is pinned |
| OpenCode | NDJSON from `run --format json`, sessionID, text/reasoning/tool_use/tool_output parts | User prompts or model names absent from run JSONL are not invented. Databases and the single JSON from `opencode export` are out of scope |

Legacy extraction keeps `isMeta` as `injected` / `origin=framework` and `isCompactSummary` as
`compacted` / `origin=compaction`. The viewer excludes both from hard scope. Extraction does not
apply the viewer's exclusions, so reasoning, internal instructions, and compaction text may remain
in the output.
