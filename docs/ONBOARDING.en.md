# LLM onboarding summary

[日本語](ONBOARDING.md) | **English**

## 1. Project and purpose

`agent-jsonl-compact` is a Rust CLI for searching, browsing, and exporting session JSONL.
It requires no LLM, network, or external agent framework at runtime.
Read `Cargo.toml` for the version, `CHANGELOG.md` for changes, and Actions or actual command results for verification status.

Extraction writes only `*.clean.jsonl` by default. `--format-out md|both` and `--summary` add files.
`view` / `search` return results to stdout without creating saved files or indexes.
A summary contains counts and paths; it is not a substitute for conversation text.

## 2. Critical requirements and constraints

Shortening message and tool-output text requires explicit options.
The viewer always excludes reasoning, system, developer, and compaction content. Extraction does not apply the same exclusions.
Never commit or send real logs, their exports, or credentials to external services.

Preserve record and display limits, partial status, and cursor contracts.
Extraction reads lines incrementally but retains all accepted events in memory. It does not guarantee bounded memory for arbitrary input.
Reject output-name traversal, input replacement, and writes through destination symlinks. Saved Unix files must be owner-only.

`Cargo.toml` defines the minimum Rust requirement; `Cargo.lock` defines the tested dependency graph.
`SKILL.md` is embedded at build time. Skill edits require rebuilding and distribution.

## 3. Reference documents

| Source | Purpose |
|---|---|
| [README](../README.en.md) / [CLI](CLI.en.md) | Setup, operations, output contracts |
| [Input formats](INPUT_FORMATS.en.md) | Provider evidence and limitations |
| [SECURITY](../SECURITY.en.md) | Check coverage and sensitive data |
| [ADR index](adr/README.en.md) | Design decisions and historical version context |
| [Viewer requirements](requirements/session_viewer_cli.md) | F01–F10 / N01–N05 and acceptance cases (Japanese) |
| [Release guide](RELEASING.en.md) | Version, tag, and asset validation |
| `tests/fixtures/README.md` | Synthetic and sanitized fixture provenance |

## 4. Task boundaries

Work covers parsers, CLI, file saving, installer, tests, and documentation.
Keep real logs under ignored paths such as `temp/`; use synthetic test fixtures.
Do not include moving published tags, publishing real logs, or changing the license in routine fixes.

## 5. Communication

Report in Japanese with the change, validation commands, and unverified scope.
Do not present an earlier passing result as a current verification.

## 6. Onboarding exercises

```bash
just check
just demo
# Expect 3 inputs × 3 output types = 9 files under demo-out/
find demo-out -type f
cargo run --locked -- view -i tests/fixtures/codex_sample.jsonl --tail 5 --output jsonl
cargo run --locked -- search -i tests/fixtures/codex_sample.jsonl --query hello --context 1
```

These are verification procedures; label them verified only after running them.
Match clean, transcript, and summary files by input stem. Confirm that clean or transcript files exist when conversation text is required.
For viewer results, inspect metadata `status`, `counts`, and `text_complete`. Exit code 0 alone does not prove the requested work is complete.

## 7. Change management

Keep usage in README, detailed contracts in CLI, and history in CHANGELOG.
Update living Japanese/English pairs together; add an ADR when a design decision changes.
For unverified statements, name the next file or command to inspect.

Before completion, verify requested artifacts, relevant tests and lint, and remaining limitations.
Preserve unrelated user changes. Do not report macOS runtime or remote Actions success before those environments have run.

### Appendix: implementation map

| Location | Responsibility |
|---|---|
| `src/cli.rs`, `src/lib.rs` | Arguments and dispatch |
| `src/detect.rs`, `src/classify.rs`, `src/clean.rs` | Extraction detection, normalization, filtering |
| `src/runner.rs`, `src/render.rs` | Saving files, installing skills, Markdown |
| `src/query/` | Bounded reader, scope, search, cursors, tool pairs |
| `tests/security.rs`, `tests/test_installer.py` | Filesystem and installer regressions |
| `.github/workflows/`, `justfile` | Quality, security, and release checks |

Run `cargo run --locked -- --help` for CLI help; inspect `Cargo.toml` / `Cargo.lock` for dependencies.
