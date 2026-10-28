# Contributor and agent guide

[日本語](ONBOARDING.md) | **English**

## 1. Project and purpose

`agent-jsonl-compact` is a Rust CLI for searching, browsing, and exporting session JSONL.
It requires no LLM, network, or external agent framework at runtime.
Read `Cargo.toml` for the current version and `CHANGELOG.md` for changes.
Check verification results in GitHub Actions for the target commit and in the output of local checks.

Extraction writes only `*.clean.jsonl` by default. `--format-out md|both` and `--summary` add files.
`view` / `search` return results to stdout without creating saved files or indexes.
A summary contains counts and paths; it is not a substitute for conversation text.

## 2. Behavior to preserve

Shortening message and tool-output text requires explicit options.
The viewer always excludes reasoning, system, developer, and compaction content. Extraction does not apply the same exclusions.
Never commit or send real logs, their exports, or credentials to external services.

Preserve input line and display limits, the conditions for `partial` status, and cursor resume and validation behavior.
`event_id` is `source_id:byte_start:subevent_index` and never includes a line number inherited from a cursor.
`source.raw_line_verified=false` marks an unverified display line number.
Extraction reads one line at a time but keeps all selected events in memory. Memory use grows with the amount of selected data.
Reject paths outside the output directory, replacement of the input file, and writes through symbolic links.
On Unix, only the owner may read and write saved files.

`Cargo.toml` defines the minimum Rust requirement; `Cargo.lock` defines the tested dependency graph.
`SKILL.md` is embedded at build time. Skill edits require rebuilding and distribution.

## 3. Reference documents

| Source | Purpose |
|---|---|
| [README](../README.en.md) / [CLI](CLI.en.md) | Setup, usage, and output format |
| [Input formats](INPUT_FORMATS.en.md) | Basis for supporting each log format and its limitations |
| [SECURITY](../SECURITY.en.md) | Check coverage and sensitive data |
| [ADR index](adr/README.en.md) | Design decisions and historical version context |
| [Viewer requirements](requirements/session_viewer_cli.md) | F01–F10 / N01–N05 and acceptance cases (Japanese) |
| [Release guide](RELEASING.en.md) | Version, tag, and asset validation |
| `tests/fixtures/README.md` | Synthetic and sanitized fixture provenance |

## 4. Scope of work

Work covers parsers, CLI, file saving, installer, tests, and documentation.
Keep real logs under ignored paths such as `temp/`; use synthetic test fixtures.
Do not include moving published tags, publishing real logs, or changing the license in routine fixes.

## 5. Communication

Report in Japanese. Describe the change, the commands used to check it, and anything still unverified.
Do not present an earlier passing result as a current verification.

## 6. Verify behavior

```bash
just check
just demo
# Expect 3 inputs × 3 output types = 9 files under demo-out/
find demo-out -type f
cargo run --locked -- view -i tests/fixtures/codex_sample.jsonl --tail 5 --output jsonl
cargo run --locked -- search -i tests/fixtures/codex_sample.jsonl --query hello --context 1
```

These are verification procedures; label them verified only after running them.
Match clean, transcript, and summary files by the input filename without its extension.
When you need conversation text, confirm that a clean or transcript file was saved.
For viewer results, inspect metadata `status`, `counts`, and `text_complete`. Exit code 0 alone does not prove the requested work is complete.

## 7. Change management

Keep usage in README, detailed behavior in the CLI reference, and history in CHANGELOG.
Update Japanese and English usage and development guides together. Add an ADR when a design decision changes.
For unverified statements, name the next file or command to inspect.

Before reporting completion, confirm that the requested files exist, run the relevant tests and lint, and note any remaining limitations.
Preserve unrelated user changes. Report macOS or remote Actions checks as passed only after they succeed in those environments.

### Source layout

| Location | Responsibility |
|---|---|
| `src/cli.rs`, `src/lib.rs` | Arguments and dispatch |
| `src/detect.rs`, `src/classify.rs`, `src/clean.rs` | Extraction detection, normalization, filtering |
| `src/runner.rs`, `src/render.rs` | Saving files, installing skills, Markdown |
| `src/query/` | Bounded reader, scope, search, cursors, tool pairs |
| `build.rs`, `src/query/version.rs` | `source_commit` and fixed source release date |
| `tests/security.rs`, `tests/test_installer.py` | Filesystem and installer regressions |
| `tests/security_hardening_e2e.rs` | F-01–F-09 resource, display, pairing, cursor, and discovery regressions |
| `tests/test_justfile.py` | `just stats` argument-injection regression |
| `.github/workflows/`, `justfile` | Quality, security, and release checks; release requires both ci and security |

Run `cargo run --locked -- --help` for CLI help; inspect `Cargo.toml` / `Cargo.lock` for dependencies.
