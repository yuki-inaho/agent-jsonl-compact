# Changelog

**English** | [日本語](CHANGELOG.ja.md)

All notable changes to `agent-jsonl-compact` are recorded here.

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
