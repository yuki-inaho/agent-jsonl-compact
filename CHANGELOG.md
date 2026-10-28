# Changelog

**English** | [日本語](CHANGELOG.ja.md)

All notable changes to `agent-jsonl-compact` are recorded here.

## 0.5.1 — 2026-10-28

### Changed

- Generalize local paths in the historical port record. This release includes the changes listed under 0.5.0, which was not published as a release.
- Clarify installation options, search roots, version metadata, and cursor conditions in the documentation.

## 0.5.0 — 2026-10-28

Source release date: **2026-10-28** (fixed release date; not a build timestamp).

### Breaking

- `event_id` is now `source_id:byte_start:subevent_index`; the unverified raw line is no longer part of the identity. Old `--event` IDs and event IDs returned by old cursors cannot be re-fetched.
- Viewer JSONL adds `source.raw_line_verified`, `matched_ranges_total`, `matched_ranges_truncated`, `pair_reason`, and `pair_state=ambiguous`. The existing five pair states are unchanged.
- Per-record normalization limits: timestamp 128 bytes, identifiers 512 bytes, phase 64 bytes, subevents 256, normalized content 2 MiB. Records above a limit become `parse_error` with `status: partial`; previously accepted oversized records are rejected.
- Source builds now require Rust 1.88; update `time` and `anyhow` past known RustSec advisories.
- `install-skills` preserves existing files; use `--force` after review to replace them. Symlinked skills are rejected.
- Extraction rejects path separators/control characters in output names, input aliases, and non-regular destinations.

### Security

- **F-01 distribution:** include the safe storage and installer work that was missing from v0.4.0. Keep output-name validation, symlink/destination/input rejection, same-directory atomic replacement, Unix 0600, existing-skill preservation, and installer tag/checksum/archive checks. Add `source_commit` and `source_commit_dirty` to `version --output json`; the release workflow now requires CI and security success for the same commit.
- **F-02 developer helper:** `just stats` uses `set positional-arguments` and `"$1"` instead of embedding the file name in a shell program.
- **F-03 normalization:** reject long timestamps/identifiers and too many subevents before draft construction, bounding amplification from an input below 1 MiB.
- **F-04 display:** route timestamps, IDs, headings, and diagnostics through `escape_controls_singleline`. Bodies keep newlines; internal JSON values stay raw.
- **F-05 resources:** cap search matches at 256 and report `matched_ranges_truncated`. If no primary event fits, fail with rc2 `budget_too_small` instead of returning rc0 with the same page cursor. Keep an absolute context-retention cap, reject FIFO/socket sources, and bound scan entries, depth, bytes, and time; search has a 30-second processing budget.
- **F-06 pairing:** use a structured provider + session + call_id key and match calls only with results. Distinguish other sessions, delimiter-containing IDs, duplicate IDs, call-only and result-only cases; duplicates become `ambiguous`.
- **F-07 cursors:** bound and validate token, arrays, strings, integers, and positions. Verify the session id against the source and never use an unverified line number in an event id. Extreme integers must not panic.
- **F-08 discovery:** when a scan limit, unreadable file, uninspected record, or invalid record remains, `view --session` / `search --session` return `session_resolution_incomplete` instead of claiming uniqueness or absence. Aggregate parse state before candidate selection.
- **F-09 extraction:** preserve `isMeta` as `injected` / `origin=framework` and `isCompactSummary` as `compacted` / `origin=compaction`. Fence Markdown message bodies so body-derived headings and external images cannot be confused with generated structure. Document that log content never grants current authority, approval, or side-effect permission.
- Save outputs through private temporary files; safely replace existing files without following destination links. Escape terminal control characters in statistics, reports, and errors.
- Validate installer tags, checksums, archive members and file types; stage installation and do not execute downloaded binaries.
- Add daily security checks and a release gate, pin Action commits, reduce release permissions, and avoid shell interpolation of event input.
- `release-verify` also runs after a successful `release` workflow (`workflow_run`), because a Release created with `GITHUB_TOKEN` does not trigger `release: published`.

### Changed

- Release tarballs now include README.en.md, SECURITY, CHANGELOG, and docs so relative links from the extracted README resolve.
- Shorten Japanese/English guides and move CLI details and input provenance into reference documents. Add `source_commit`, `raw_line_verified`, match truncation, pair `ambiguous`, `session_resolution_incomplete`, and scan limits to the CLI reference.
- `version --output json` refreshes `source_commit` and `source_commit_dirty` on incremental builds, including linked worktrees. When Git is unavailable, the dirty state is `null` (unknown) instead of clean.
- Record the new decisions in ADR 0007.

## 0.4.0 — 2026-10-23

Source release date: **2026-10-23** (fixed release date; not a build timestamp).

### Breaking

- Extraction now writes only `<name>.clean.jsonl` by default. Pass `--format-out md|both`
  to write `<name>.transcript.md`, and `--summary` to write `<name>.summary.json`.
  The clean schema, other extraction options, and exit codes are unchanged.

### Added

- `--summary` opt-in flag for summary JSON output.
- Native macOS E2E: `cargo test` runs on both `macos-latest` (arm64) and
  `macos-15-intel` (x86_64), followed by a release-binary smoke test.
- `tests/fixtures/claude_desktop_sample.jsonl`: a sanitized real Claude Code / Desktop
  transcript with text, thinking, tool calls/results, and a pending final tool call.
  Provenance and the sanitization policy are in `tests/fixtures/README.md`.
- `scripts/smoke-release.sh`: exercises `--version`, `version --output json`, `--stats`,
  `view`, `search`, and extraction against the release binary.
- `release-verify.yml`: downloads published Linux/macOS release assets, verifies their
  checksums, and runs the release binary on native runners.

### Changed

- Release workflow builds the Intel macOS asset on `macos-15-intel` natively, and runs
  the release-binary smoke test before packaging.
- `weekly-release-check.sh` no longer classifies tests/docs/CI-only changes as requiring
  a release.

## 0.3.1 — 2026-10-23

Source release date: **2026-10-23** (fixed release date; not a build timestamp).

### Added

- Prebuilt macOS artifacts: `aarch64-apple-darwin` (Apple Silicon) and
  `x86_64-apple-darwin` (Intel), each published as `.tar.gz` + `.sha256`.
- `install.sh` detects Linux x86_64 musl and macOS arm64/x86_64, and verifies
  checksums with `sha256sum` or macOS `shasum -a 256`.

### Changed

- `release.yml` builds a Linux/macOS matrix and publishes all assets to one GitHub Release.
- CI builds both macOS targets on every push and pull request.
- Added [ADR 0004](docs/adr/0004-versioning-and-release-policy.md) (versioning and release
  policy) and [ADR 0005](docs/adr/0005-bilingual-documentation.md) (bilingual docs policy).
- README / ONBOARDING / RELEASING / ADR index now have Japanese/English pairs, with
  Japanese as the default and English in `*.en.md`.
- `just weekly-check` / `scripts/weekly-release-check.sh` reports changes since the latest
  tag and whether a release is needed; `just docs-check` verifies doc pairs.

### Notes

- No CLI behavior, output schema, cursor version, or query schema changes since 0.3.0.
- macOS binaries are ad-hoc signed by the toolchain and are not notarized; see README.
- The `SKILL.md` update merged after v0.3.0 is embedded in 0.3.1 binaries.

## 0.3.0 — 2026-10-21

Source release date: **2026-10-21** (fixed release date; not a build timestamp).

### Added

- `sessions` — find conversation candidates under declared provider roots
  (`--root`, `--provider`, `--cwd`, `--cwd-children`, `--scan-limit`).
- `view` — read a session or a file with bounded output, filters
  (`--scope`, `--role`, `--phase`, `--since`, `--until`), `--tail`, `--event`,
  `--with-pair`, and page/append cursors (`--cursor`, `--after-cursor`).
- `search` — literal (OR) or opt-in `--regex` search with `--context`, returning
  `matched_queries` and `matched_ranges` against the original text.
- `version --output json` — machine-readable version, fixed `source_release_date`,
  and query schema version.
- Viewer output carries provenance: raw line, byte range, subevent index, provider
  session id, message origin, tool pair state and coverage counters.

### Compatibility

- Legacy extraction (`-i/-o`, `--format-out`, `--channel`, `--name`, `--stats`,
  `--no-dedup`, `--elide-outputs`, `--msg-chars`, `--out-chars`) keeps its existing
  schema, defaults and exit codes.
- New viewer commands use their own exit contract: `0` processed (including a
  legitimate empty result), `1` source/decode/format-uncertain/partial/cursor error,
  `2` argument or query syntax error.

### Notes

- Query commands never write to the source and create no cache or index by default.
- Memory is bounded by the requested window, the per-record cap and the output
  budget, not by the number of events in the input.
- Cursor mode requires Unix file-generation identity (`dev`+`inode`); other platforms
  reject it with `cursor_unsupported_platform`.
