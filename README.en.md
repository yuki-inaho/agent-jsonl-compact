# agent-jsonl-compact

[日本語](README.md) | **English**

[![ci](https://github.com/yuki-inaho/agent-jsonl-compact/actions/workflows/ci.yml/badge.svg)](https://github.com/yuki-inaho/agent-jsonl-compact/actions/workflows/ci.yml)

`agent-jsonl-compact` is a single Rust CLI that auto-detects OpenAI Codex CLI rollout
JSONL, Anthropic Claude Code transcript JSONL, and OpenCode `run --format json` NDJSON,
then extracts only the conversation, reasoning, and tool execution into compact JSONL /
Markdown / summary JSON. It ships both a CLI for reading huge session logs in stages
("compact first, then read") and the agent skill (`agent-jsonl-compact-reader`) that uses it.

Version 0.3.0 kept the existing extraction behavior and added `sessions` / `view` /
`search` for finding a conversation and reading only the needed parts
([ADR 0001](docs/adr/0001-additive-session-viewer-cli.md)). Extraction is "save then read";
the viewer commands are "return to stdout and read".

Version 0.3.1 is a distribution and documentation release. It adds prebuilt macOS
binaries (Apple Silicon and Intel) and changes the release workflow to a Linux/macOS
matrix. It also adds the versioning policy
([ADR 0004](docs/adr/0004-versioning-and-release-policy.en.md)) and the bilingual
documentation policy ([ADR 0005](docs/adr/0005-bilingual-documentation.en.md)). The CLI
behavior, output schema, and cursor version are unchanged from 0.3.0.

The original implementation is the following Python script (separate repository):

```text
skills/session-transcript-extractor/scripts/extract_session_jsonl.py
```

## Documentation

- [Documentation index (English)](docs/README.en.md) / [ドキュメント索引 (日本語)](docs/README.md)
- [LLM onboarding (English)](docs/ONBOARDING.en.md) / [LLM オンボーディング (日本語)](docs/ONBOARDING.md)
- [Releasing and versioning (English)](docs/RELEASING.en.md) / [リリースとバージョニング (日本語)](docs/RELEASING.md)
- [ADR index (English)](docs/adr/README.en.md) / [ADR 索引 (日本語)](docs/adr/README.md)

## Install (prebuilt)

No Rust/cargo required. The prebuilt binaries from GitHub Releases are installed into
`~/.local/bin`. `install.sh` detects the OS/arch, verifies the matching tarball with
sha256, and extracts it.

```bash
curl -fsSL https://raw.githubusercontent.com/yuki-inaho/agent-jsonl-compact/main/install.sh | bash
```

| Platform | target | archive |
|---|---|---|
| Linux x86_64 | `x86_64-unknown-linux-musl` | `agent-jsonl-compact-x86_64-unknown-linux-musl.tar.gz` |
| macOS Apple Silicon | `aarch64-apple-darwin` | `agent-jsonl-compact-aarch64-apple-darwin.tar.gz` |
| macOS Intel | `x86_64-apple-darwin` | `agent-jsonl-compact-x86_64-apple-darwin.tar.gz` |

Each archive contains `README.md` / `LICENSE`, and each release provides a separate
`.sha256` file. For other environments, use [Build from source](#build-from-source).

To pin a version or change the install directory, pass environment variables to the
`bash` on the receiving side of the pipe:

```bash
curl -fsSL https://raw.githubusercontent.com/yuki-inaho/agent-jsonl-compact/main/install.sh \
  | AJC_VERSION=v0.3.1 INSTALL_DIR=~/bin bash
```

| Variable | Meaning | Default |
|---|---|---|
| `AJC_VERSION` | release tag to fetch | latest release |
| `INSTALL_DIR` | install directory | `~/.local/bin` |

macOS binaries are only ad-hoc signed by the toolchain and are not notarized. If
Gatekeeper quarantines an archive saved through a browser, run:

```bash
xattr -d com.apple.quarantine ~/.local/bin/agent-jsonl-compact
```

`install.sh` verifies checksums with either Linux `sha256sum` or macOS `shasum -a 256`.

## Skill: agent-jsonl-compact-reader

This is a Claude Code / Codex skill for reading existing session JSONL in stages
("compact first, then read"). It uses `summary.json` to understand the overall size, then
reads only the necessary parts of `transcript.md` / `clean.jsonl`, avoiding loading a huge
raw log into context. `SKILL.md` is embedded in the binary.

After installing the binary, **the CLI itself installs the skill** for each agent
(the same approach as Microsoft `playwright-cli install --skills`).

```bash
agent-jsonl-compact install-skills              # both ~/.claude and ~/.codex (only if present)
agent-jsonl-compact install-skills --claude-only
agent-jsonl-compact install-skills --codex-only
```

Destinations:

```text
~/.claude/skills/agent-jsonl-compact-reader/SKILL.md
~/.codex/skills/agent-jsonl-compact-reader/SKILL.md
```

Combined with the prebuilt installer, this is a one-liner:

```bash
curl -fsSL https://raw.githubusercontent.com/yuki-inaho/agent-jsonl-compact/main/install.sh | bash \
  && ~/.local/bin/agent-jsonl-compact install-skills
```

To edit the repository `SKILL.md` directly while developing, you can also register it
with symlinks:

```bash
ln -sfn "$PWD/skills/agent-jsonl-compact-reader" ~/.claude/skills/agent-jsonl-compact-reader
ln -sfn "$PWD/skills/agent-jsonl-compact-reader" ~/.codex/skills/agent-jsonl-compact-reader
```

## Usage

Extraction (default behavior):

```bash
agent-jsonl-compact -i <path/to/session.jsonl> -o <out_dir>

agent-jsonl-compact -i <path/to/session.jsonl> --stats

agent-jsonl-compact \
  -i ~/.codex/sessions/2026/06/07/rollout-xxx.jsonl \
  -o temp/session_extracts \
  --msg-chars 4000 \
  --out-chars 2000
```

OpenCode normally stores sessions in `~/.local/share/opencode/opencode.db` and does not
create JSONL automatically. The supported input is the stdout of `run --format json`
saved to a file.

```bash
opencode run --format json "<prompt>" > opencode-session.jsonl
agent-jsonl-compact -i opencode-session.jsonl -o temp/session_extracts
```

This stream contains `step_start` / `text` / `reasoning` / `tool_use` / `step_finish` /
`error`. The user prompt and model name are not emitted, so the extractor does not guess
them. `opencode export` produces a single JSON document, which is a different format from
this JSONL input.

### Input format sources and stability

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

Subcommands / information:

```bash
agent-jsonl-compact install-skills [--claude-only|--codex-only]
agent-jsonl-compact --version
agent-jsonl-compact version --output json
agent-jsonl-compact --help
```

Defaults prioritize fidelity. `--msg-chars 0` and `--out-chars 0` mean "keep everything".
Specify positive values only when you need to reduce size further.

## Outputs

`<name>` is the input file stem, or the value passed to `--name`.

```text
<name>.clean.jsonl
<name>.transcript.md
<name>.summary.json
```

Even when `--format-out jsonl` or `--format-out md` is specified, summary JSON is always
written.

## Options (extraction)

```text
--format auto|codex|claude_code|opencode
--channel terminal|api|both      # Codex only
--msg-chars N                    # 0 keeps full text
--out-chars N                    # 0 keeps full output
--elide-outputs                  # fold large tool outputs into counts and first lines
--keep-token-count               # keep Codex token_count
--no-dedup                       # disable duplicate folding
--format-out jsonl|md|both
--stats                          # print format and record type distribution, then exit
--name <stem>                    # output base name (default: input stem)
```

## Viewer CLI (0.3.0): find → read → return to evidence

Unlike extraction, which writes files, `sessions` / `view` / `search` are viewer commands
that **return to stdout without saving**. They never modify the source and create no cache
or index by default. See [ADR 0001](docs/adr/0001-additive-session-viewer-cli.md) /
[ADR 0002](docs/adr/0002-cursor-and-resource-boundaries.md) for the design decisions.

Shortest path:

```bash
agent-jsonl-compact sessions --provider codex --cwd /path/to/repo
agent-jsonl-compact view --session codex:SESSION_ID --tail 30 --output jsonl
agent-jsonl-compact search -i /path/rollout.jsonl --query keyword --context 2
```

### Version and update date

```bash
agent-jsonl-compact --version            # semver only (unchanged)
agent-jsonl-compact version --output json
```

`version --output json` returns `version`, the fixed `source_release_date`,
`query_schema_version`, and `cursor_version`. The update date is the **fixed source
date**, not the build time or the GitHub Release update time
([ADR 0003](docs/adr/0003-version-and-release-metadata.md)).

### 1. Find candidates — `sessions`

```bash
# exact cwd match (default). Add --cwd-children to include child directories.
agent-jsonl-compact sessions --provider codex --cwd /path/to/repo --limit 20

# include Claude Code / declare an explicit search root
agent-jsonl-compact sessions --provider all --root ~/.claude/projects --output jsonl
```

- Only provider-specific known roots and roots explicitly passed with `--root` are
  searched. **OpenCode run JSONL is not searched under the default roots** (the CLI does
  not guess `opencode.db`); pass `--root` explicitly to use it.
- `--provider all` targets codex / claude_code / opencode.
- If `--scan-limit` (default 10000) is reached, the result reports `status: partial` and
  the unscanned remainder.
- Output reports the start time (`started_at`) and the last event time
  (`last_event_at`) separately. mtime is not used to decide whether a session is running
  or stopped.

### 2. Read the needed part — `view`

```bash
# last 30 events (tail after filtering, in source order)
agent-jsonl-compact view --session codex:SESSION_ID --tail 30

# file input + time range + role
agent-jsonl-compact view -i /path/rollout.jsonl \
  --since 2026-10-21T00:00:00+09:00 --role user,assistant --output jsonl

# re-read one event (display limits and text_complete are preserved)
agent-jsonl-compact view -i /path/rollout.jsonl --event EVENT_ID --output jsonl
```

- The default scope is `messages`. `--scope tools` selects tools; even `--scope all`
  never exposes private reasoning, system, developer, or compaction content.
- The default display count is 50 events; `--max-events` (primary + context + pair total)
  defaults to 200.
- `--with-pair` adds the tool result matching the selected tool call by `call_id`. States
  are `paired` / `pending_result` (confirmed absent after a complete scan) /
  `unknown_pair` (scan incomplete, absence not provable) / `orphan` (result without a
  call) / `not_rendered_budget` (exists but not displayed due to budget).
- Pair counts are split into `paired_total` (true total) and `paired_event_ids` (retained
  IDs). On truncation, `paired_ids_truncated: true` and `pair_reentry` give the re-entry
  position. Do not read an array length as the counterpart total.
  `scan.with_pair.incomplete` means the pair scan itself was incomplete.
- `text_complete: false` means "the display was cut"; it does not mean the retrieved
  range is incomplete.

### 3. Search the body — `search`

```bash
agent-jsonl-compact search -i /path/rollout.jsonl \
  --query contour_v2 --query "エラー" --context 2 --limit 10

# regex only when explicitly requested
agent-jsonl-compact search -i /path/rollout.jsonl --query 'timeout|retry' --regex
```

- Repeating `--query` is **OR**. Each hit carries `matched_queries`.
- `matched_ranges` are **UTF-8 byte offsets into the original text**. They refer to the
  source even when the displayed text is cut.
- `--event EVENT_ID` only narrows the target events; it does not guarantee full text
  under the same budget. If you need full text, raise `--max-output-bytes` (and
  `--max-record-bytes` if needed), or read the raw range via `source.byte_start` /
  `byte_end`.
- `--context K` adds K events before and after within the hard scope. Neighboring events
  that cross role/time soft filters are marked `is_context: true`, but reasoning /
  developer content is never included.
- If an event is both a hit and context for another hit, it is emitted **once as
  primary**.

### 4. Evidence and source positions

Each event line carries:

| field | meaning |
|---|---|
| `event_id` | stable ID built from source identity + raw start position + subevent |
| `source.raw_line` | raw line number (1-based) |
| `source.byte_start` / `byte_end` | raw byte range (0-based, end exclusive) |
| `source.subevent_index` | index when one raw line expands to multiple events |
| `text_complete` | whether the displayed text is complete |
| `message_origin` | `human` / `framework` / `compaction` / `unknown`. **A user role is not necessarily human** |

### 5. Read only appends — page cursor and append cursor

```bash
# continue the same fixed snapshot (page_cursor from previous metadata)
agent-jsonl-compact view -i /path/rollout.jsonl --cursor 'c1:...' --output jsonl

# only appends after a completed scan (append_cursor from previous metadata)
agent-jsonl-compact view -i /path/rollout.jsonl --after-cursor 'c1:...' --output jsonl
```

| | page cursor | append cursor |
|---|---|---|
| purpose | continue the same snapshot | appends after the previous complete line end |
| issued when | primary events remain unread | every line was scanned with no uninspected gap |
| caution | append cursor is not issued mid-page | never advances past an uninspected record |

- A cursor preserves the filter / query / scope at issue time, plus search `--context`,
  and reuses them on resume. If omitted, the cursor value is used; if explicitly
  specified with a different value, the result is `filter_mismatch`.
  **An explicitly different filter yields `cursor_invalid` (rc1, reason
  `filter_mismatch`); specifying the same value is not a conflict.**
- `--limit` / `--tail` / `--max-events` / `--max-output-bytes` are display conditions and
  are not part of cursor identity. Appends can still be read after a tail with the same
  filter.
- Replacement (`generation_mismatch`), truncation (`truncated`), and boundary rewrite
  (`anchor_mismatch`) stop with typed errors (`cursor_invalid`, rc1). There is no implicit
  re-read from the beginning.
- If an observable change happens during reading, the command returns `status: partial`
  and `source_changed: true` with rc1, and does not issue an append cursor (no silent
  completion).
- Cursors assume append-only input. They do not guarantee full immutability of a prefix
  outside the visible range.
- Cursor mode uses Unix file generation (`dev`+`inode`). Non-Unix platforms are rejected
  explicitly with `cursor_unsupported_platform`.

### 6. Exit codes and output separation

| rc | meaning |
|---|---|
| `0` | target range processed normally (including a legitimate empty result) |
| `1` | source / decode / undetermined format / partial / invalid cursor / change during read |
| `2` | argument or query syntax error (regex compile failure, too-small budget, misuse of legacy flags, etc.) |

- JSONL output is "event lines + exactly one metadata line at the end". Diagnostics and
  progress go to **stderr**; stdout never contains prose.
- Zero results are confirmed only when every target was scanned. If an unscanned range
  remains, the result is `partial`.
- Text output escapes control characters such as ESC / OSC / C1. Commands inside tool
  output are never executed.

### 7. Budgets and limits

- `--max-output-bytes` (default 32768, minimum 8192) is the **combined stdout + stderr**
  budget, including metadata, titles, and diagnostics. If it does not fit, the command
  rejects with `budget_too_small` (rc2) and never emits a truncated invalid JSON.
- Lines larger than `--max-record-bytes` (default 1 MiB) are returned as `uninspected`
  with only their raw range, `status: partial` / rc1, and no `append_cursor`. Use
  `counts.uninspected.samples[].retry_cursor` in metadata to re-read from the same
  position with a larger limit.
- Input record limits and display limits are separate. A display cut is
  `text_complete: false`, not partial.

### 8. Reading the read-byte counters

Metadata `scan` contains:

| field | meaning |
|---|---|
| `scan.read_bytes` | source bytes logically consumed by the main scan |
| `scan.total_read_bytes` | main + auxiliary scans (format detection, metadata search, anchor validation, pairing) |
| `scan.read_bytes_breakdown.main` / `.detect` / `.metadata` / `.anchor` / `.validation` / `.pair` | breakdown of the above |
| `scan.with_pair.passes` / `scan.with_pair.read_bytes` | pair auxiliary scan passes and bytes |

These are **logical consumed bytes**, not OS physical I/O or speed. Auxiliary scans can
make `total_read_bytes` exceed the file size. With `-i` plus a cursor, the command reuses
provider/session from the cursor and reads only the anchor neighborhood and the append,
but `--with-pair` scans the same snapshot up to two extra times, so that bound does not
apply (see the pair-read column in the measurement table). `--session` also performs a
catalog search before using the cursor, so each call may pay that search cost; it does not
guarantee O(suffix) for appends.

### 9. Limited measurement (synthetic input)

This example measures a synthetic input generated with seed 42 (`claude_code`, 110,088
records, 69,234,347 B, input sha256 `0c64ec59…a74f39a`) with a **candidate release binary
before the context/regex profile fix** (sha256 `bc52b88d…4ed3ea`, linux). The **verified
candidate binary** for the final frozen source is sha256 `7ee5e42b…` (source-side runner
`61f297b5…`) and was verified by independent tests, but it is not the binary measured in
this table. **This is not a general performance proof, nor a claim that the two binaries
are identical.**

A receipt records the seed, generation parameters, record count, input sha256,
`binary_sha256`, debug/release, `rustc` version, platform, and cache conditions.

| Case | Main scan read | total read | wall | peak RSS | output |
|---|---|---|---|---|---|
| view first 50 | 23,603 B | 174,590 B | 0.0041 s | 4,652 KiB | 29,209 B |
| search all, 1 hit at end | 69,234,347 B | 69,385,462 B | 0.6577 s | 4,736 KiB | 3,076 B |
| view --scope tools --with-pair | 179,464 B | 138,799,145 B | 2.1657 s | 4,736 KiB | 30,848 B |
| append (3 lines) | 579 B | 1,413 B | 0.0027 s | 4,268 KiB | 3,853 B |

All values are **logical consumed bytes**, not OS physical I/O. Cold/warm state was not
controlled and is treated as unknown. The `--with-pair` total-read increase is due to the
designed maximum of two extra passes. The population is one synthetic shape; this does
not generalize to 1 GB inputs or real conversations.

Reproduce:

```bash
cargo build --release
python3 scripts/benchmark_viewer.py generate --out temp/viewer_bench/input.jsonl
python3 scripts/benchmark_viewer.py measure \
  --input temp/viewer_bench/input.jsonl --out-dir temp/viewer_bench/out
```

### 10. Legacy extraction still works

The viewer addition does not replace legacy extraction. Existing commands keep their
schema, defaults, and exit codes.

```bash
agent-jsonl-compact -i /path/rollout.jsonl -o out --format-out both
```

Passing a legacy flag to a viewer command, such as `view -i FILE --format-out md`,
**returns rc2** (use `--format` for input format and `--output text|jsonl` for output
representation).

## Build from source

```bash
cargo test
cargo build --release            # -> target/release/agent-jsonl-compact
just install                     # install the local build into ~/.local/bin
```

To reproduce the distributed musl static binary and tarball locally:

```bash
just build-musl                  # musl static binary
just dist                        # tarball + sha256 in dist/ (same as CI)
```

The release workflow builds both `aarch64-apple-darwin` and `x86_64-apple-darwin` for
macOS. To check locally, run `cargo build --release` for the host arch, or add the target
with `rustup target add` and pass `--target` for a cross build.

## Tasks (justfile)

```bash
just                 # task list (= just --list)
just build           # build the release binary
just test            # all tests (unit + integration)
just check           # fmt-check + clippy(-D warnings) + test + docs-check (= CI gate)
just demo            # build -> run examples against the three bundled fixtures
just stats <f>       # show only format and record type distribution
just build-musl      # build the musl static binary (for distribution)
just dist            # generate distribution tarball + sha256
just install         # install the local build into ~/.local/bin
just install-release # install the prebuilt via the local install.sh
just install-skills  # install the reader skill for each agent
just docs-check      # check Japanese/English doc pairs and cross-links
just weekly-check    # compare against the latest tag and report release need
```

## S/N (signal vs noise) policy

The default is a faithful mode: "remove only true noise, never remove signal".

- Remove (pure noise): Codex `token_count` (telemetry) / injected system instructions /
  redundant channels / Claude Code `ai-title` / `mode` / `attachment`, etc. / consecutive
  identical duplicates.
- Keep (signal): full message text (`--msg-chars 0`) / full tool output
  (`--out-chars 0`) / incremental commentary / every tool call.
- OpenCode: split completed `tool_use` into `tool_call` + `tool_output`, and preserve
  `step_finish` `tokens` / `cost` / `reason` in `turn_end`.
- Add lossy knobs (`--msg-chars N` / `--out-chars N` / `--elide-outputs` /
  `--channel api`) explicitly only when size is the priority.

## Safety / PII

Real session JSONL and its extracts (`*.clean.jsonl` / `*.transcript.md`) contain
conversation text, home paths, and project contents (PII). **Do not commit extracts**
(already in `.gitignore`). Bundled fixtures contain synthetic data only.

## pyo3 (future / optional)

The primary distribution is a binary. To leave room for calling directly from Python in
the future, the project keeps the option of exposing a thin `lib.rs` wrapper via `pyo3`
(not provided in this version). For now, use the CLI binary through a subprocess.

## Development layout

```text
src/main.rs       entry point (only calls run_cli)
src/lib.rs        module exports, run_cli, public API (run / install_skills_into)
src/cli.rs        CLI options and argument definitions for extraction and query subcommands
src/detect.rs     Codex / Claude Code / OpenCode auto-detection (legacy extraction)
src/classify.rs   expand raw JSON records into normalized events
src/clean.rs      keep set, truncate, dedup, elide
src/render.rs     Markdown rendering
src/counter.rs    raw record type / normalized kind counters
src/runner.rs     extraction control, aggregation, summary, file output, install-skills
src/util.rs       JSONL streaming and JSON helpers

src/query/reader.rs     bounded physical line reader (snapshot boundary, record limit, pending/invalid)
src/query/cursor.rs     page / append cursor encoding and validation
src/query/detect.rs     viewer provider detection (fail closed)
src/query/normalize.rs  provider normalization (origin / private / mirror identification)
src/query/select.rs     hard scope and soft filters
src/query/filter.rs     role / phase / time filters
src/query/toolpair.rs   call_id pairing (pending / orphan / budget states)
src/query/source.rs     session discovery, source identity, metadata extraction
src/query/sessions.rs   `sessions` candidate enumeration
src/query/output.rs     text / JSONL serialization
src/query/runner.rs     viewer control, budget, metadata
src/query/version.rs    single source of version and fixed source release date

docs/adr/                        design records (0001 viewer CLI / 0002 cursor and resources / 0003 versioning / 0004 version policy / 0005 bilingual docs)
docs/requirements/               requirements and acceptance cases
docs/README.md / README.en.md    documentation index (Japanese / English)
docs/ONBOARDING.md / .en.md      LLM onboarding (Japanese / English)
docs/RELEASING.md / .en.md       weekly check and release procedure (Japanese / English)
docs/adr/README.md / .en.md      ADR index (Japanese / English)
CHANGELOG.md / CHANGELOG.ja.md   changelog (English canonical / Japanese)
skills/agent-jsonl-compact-reader/SKILL.md  reader skill (embedded into the binary at build time)
tests/fixtures/*.jsonl                      synthetic fixtures for the three formats (no real sessions)
tests/viewer_e2e.rs                         A01–A12 acceptance E2E
tests/viewer_integrity_e2e.rs               concurrent-read integrity E2E (Linux)
tests/viewer_support/mod.rs                 isolated HOME/XDG and fixture generation
scripts/benchmark_viewer.py                 synthetic input generation and limited measurement
scripts/weekly-release-check.sh             diff and release-need report since the latest tag
scripts/check-docs.sh                       Japanese/English doc pair and cross-link check
.github/workflows/ci.yml       fmt-check + clippy + test / macOS 2-target release build
.github/workflows/release.yml  on v* tags, publish Linux musl + macOS (arm64/x86_64) tarballs + sha256
install.sh                     prebuilt installer (Linux/macOS, sha256 verification)
```

> `SKILL.md` is embedded into the binary with `include_str!`. Rebuild after changing the
> skill text.
