# LLM onboarding summary

[日本語](ONBOARDING.md) | **English**

> This template is a starting point to share with a new LLM agent joining the project. Fill in each item and add source documents or supporting links where available.

## 1. Project overview and purpose
- **Project name / domain:** `agent-jsonl-compact` — a single Rust CLI that compacts session JSONL from Codex CLI / Claude Code / OpenCode run, plus the agent skill that uses it.
- **Final deliverables:** a single binary that auto-detects raw JSONL and by default writes only `*.clean.jsonl`. Passing `--format-out md|both` also writes `*.transcript.md`, and `--summary` also writes `*.summary.json`. It also provides the `sessions` / `view` / `search` viewer CLI, the reader skill (`agent-jsonl-compact-reader`), and prebuilt binaries for Linux x86_64 musl / macOS arm64 / macOS x86_64 (`install.sh` / GitHub Releases).
- **Business context / value:** avoid loading huge session logs raw into context and save tokens by "compact → read in stages". It is used as a CLI on `PATH` by surrounding tools such as `slack-knowledge-rag` (`just extract-session` / `inspect-session`).
- **Current progress summary:** `v0.4.0` (2026-10-23) is released. `0.3.0` added `sessions` / `view` / `search` with cursors and provenance; `0.3.1` added macOS prebuilt binaries, the release matrix, ADR 0004/0005, and bilingual documentation; `0.4.0` changed the default extraction output to clean JSONL only (`--format-out md|both` / `--summary` opt in to transcript/summary). `Verified`: during release preparation, `just check` (fmt / clippy `-D warnings` / 54 unit + 7 integration + 37 viewer E2E + 4 integrity = 102 tests) and `just docs-check` passed locally. macOS runs native `cargo test` plus a release-binary smoke test on `macos-latest` (arm64) and `macos-15-intel` (x86_64). For current CI and Release state, see GitHub Actions / Releases.

## 2. Critical requirements and constraints
> List the quality and specification lines that must not be broken.
- The default is **faithful mode**. Signal (all message text, all tool output, every tool call) is not removed. Lossy knobs (`--msg-chars` / `--out-chars` / `--elide-outputs` / `--channel api`) only take effect when explicitly specified.
- Real session JSONL and extracts (`*.clean.jsonl` / `*.transcript.md` / `*.summary.json`) contain conversation text, home paths, and other **PII. Do not commit them** (already in `.gitignore`). The three format samples are synthetic; `claude_desktop_sample.jsonl` is a sanitized real conversation. See `tests/fixtures/README.md` for removed fields.
- JSONL is **streamed**; the entire raw log is not loaded into memory (`src/util.rs`). Only selected events are retained.
- Do not break the format auto-detection priority (`src/detect.rs`) or the normalized event kinds / keep set (`src/classify.rs` / `src/clean.rs`).
- Distribution targets are **Linux x86_64 musl static / macOS arm64 / macOS x86_64**. `install.sh` fails explicitly for other OS/arch combinations.
- Cursor mode uses Unix file generation (`dev`+`inode`). Non-Unix platforms are rejected explicitly with `cursor_unsupported_platform`.
- `SKILL.md` is embedded into the binary with `include_str!` at build time. **Rebuild after changing the skill text.**
- Releases go through `v*` tags → `release.yml`. Linux and two macOS targets are built in a matrix and attached to one Release. The repository is public.

## 3. Agreed reference material
> Primary sources a new agent must check. Paths and roles are listed.
| Kind | File / link | Summary / use |
|------|------------------|------------|
| Overview / usage | `README.md` / `README.en.md` | Install (Linux/macOS) / usage / viewer CLI / options / S/N policy / development layout (Japanese/English) |
| Documentation index | `docs/README.md` / `docs/README.en.md` | Entry point for all docs (Japanese/English) |
| Changelog | `CHANGELOG.md` / `CHANGELOG.ja.md` | Per-version changes and fixed source release date (English canonical + Japanese) |
| Release procedure | `docs/RELEASING.md` / `docs/RELEASING.en.md` | Weekly check, version decision, tag / Release / asset verification (Japanese/English) |
| Versioning policy | `docs/adr/0004-versioning-and-release-policy.md` | Applying SemVer, 0.x handling, patch/minor/major classification (Japanese/English) |
| Bilingual docs policy | `docs/adr/0005-bilingual-documentation.md` | Japanese default, `*.en.md`, sync and checks (Japanese/English) |
| Design decisions | `docs/adr/0001-additive-session-viewer-cli.md` / `0002-cursor-and-resource-boundaries.md` / `0003-version-and-release-metadata.md` | Adding the viewer CLI, cursors and resource boundaries, version/date/release contract |
| ADR index | `docs/adr/README.md` / `docs/adr/README.en.md` | List of all ADRs with English summaries (Japanese/English) |
| Requirements / acceptance | `docs/requirements/session_viewer_cli.md` | F01–F10 / N01–N05 and acceptance cases |
| Design / work record | `docs/workdoc_session_jsonl_compactor_rust_port.md` | Porting rationale, module mapping, distribution/skill integration (chapter 10), DOD |
| Skill specification | `skills/agent-jsonl-compact-reader/SKILL.md` | Staged-reading flow for the reader skill (summary → read only what is needed) |
| OpenCode CLI spec | <https://dev.opencode.ai/docs/cli/#run> | Official CLI contract for `run --format json`, `export`, `db path` |
| OpenCode JSONL implementation | <https://github.com/anomalyco/opencode/blob/v1.18.26/packages/opencode/src/cli/cmd/run.ts> | Basis for the JSONL shape and event types emitted to stdout |
| OpenCode part types | <https://github.com/anomalyco/opencode/blob/v1.18.26/packages/sdk/js/src/v2/gen/types.gen.ts> | Field definitions for tool state, tokens, cost, etc. |
| Codex rollout writer (0.152.0) | <https://github.com/openai/codex/blob/rust-v0.152.0/codex-rs/rollout/src/recorder.rs> | Same tag as local `codex-cli 0.152.0`. Basis for the JSONL writer, storage location, `timestamp` / `ordinal`, and flattened `RolloutItem` |
| Codex rollout compatibility tests (0.152.0) | <https://github.com/openai/codex/blob/rust-v0.152.0/codex-rs/rollout/src/tests.rs> | Examples of synthetic `session_meta`, `event_msg`, `response_item` records and JSON compatibility |
| Claude Code session storage | <https://code.claude.com/docs/en/sessions#where-transcripts-are-stored> | `~/.claude/projects/<project>/<session-id>.jsonl`; each line is a message/tool/metadata JSON object. Also the source for the constraint that the in-line format is internal and may change |
| Claude Code application data | <https://code.claude.com/docs/en/claude-directory> | Basis for `~/.claude` containing transcripts and other application data, and for treating it as PII |
| Task definitions | `justfile` | build / test / check / demo / dist / install* / bench-viewer / docs-check / weekly-check / smoke-release |
| Test assets | `tests/integration.rs`, `tests/viewer_e2e.rs`, `tests/viewer_integrity_e2e.rs`, `tests/fixtures/*.jsonl`, `tests/fixtures/README.md` | Extraction, install-skills, viewer CLI A01–A12, concurrent-read integrity, and the sanitized real Claude transcript (synthetic fixtures plus provenance) |
| CI / Release | `.github/workflows/ci.yml`, `.github/workflows/release.yml`, `.github/workflows/release-verify.yml` | fmt / clippy / test + docs check, native macOS arm64/Intel tests and release builds, `v*` tag publication, post-release asset verification |
| Installer | `install.sh` | OS/arch detection, sha256 verification, install into `~/.local/bin` |
| Runtime smoke | `scripts/smoke-release.sh` | version / stats / view / search / extraction against a release binary |
| Known issues | GitHub Issues | No dedicated issue list yet. Check Issues / Actions / Releases for current state |

## 4. Task boundaries (delegate / do not delegate)
### Tasks to delegate (examples)
- New formats / new record types (`src/detect.rs` / `src/classify.rs`).
- CLI option and output rendering improvements (`src/cli.rs` / `src/render.rs`).
- CI, distribution, `install.sh`, skill improvements, and added tests (`tests/`).
- Documentation updates (README / workdoc / SKILL.md / Japanese-English pairs).

### Tasks not to delegate (examples)
- Committing or externally sending real session data or extracts (PII).
- Breaking changes to the faithful-mode default (e.g. removing message text / tool output by default).
- Changing public/private visibility or making destructive releases (moving existing tags, etc.) without approval.
- Unapproved changes to license or copyright notices.

## 5. Interaction policy
- **Response style:** Japanese by default. Keep technical terms and code identifiers in their original form. Use headings + bullets.
- **Response procedure:** premises → issues → proposal. Keep changes minimal and include verification command output.
- **Prohibitions / cautions:** Do not state unconfirmed things as fact. Distinguish `verified` / `untested` / `estimated`.
- **Handling secrets:** Do not include PII, tokens, or conversation text in output or commits. Put extracts in ignored locations such as `temp/`.

## 6. Trial tasks (onboarding exercises)
> List 2–3 small verification tasks to check understanding.
1. Run `just check` and `just demo`, then read the three outputs in `demo-out/` (`*.summary.json` / `*.transcript.md` / `*.clean.jsonl`) (`verified` working).
2. Use `agent-jsonl-compact -i tests/fixtures/opencode_sample.jsonl --stats` to check OpenCode auto-detection and the record type distribution.
3. Run `agent-jsonl-compact view -i tests/fixtures/codex_sample.jsonl --tail 5` and `search -i tests/fixtures/codex_sample.jsonl --query <word> --context 1`, then read `source` / cursor / `status` in the metadata.
4. Run `agent-jsonl-compact version --output json` and check `version` / `source_release_date` / `query_schema_version` / `cursor_version`.
5. In a temporary HOME, run `HOME=$(mktemp -d ...) agent-jsonl-compact install-skills` and verify placement under `.claude` / `.codex` (do not pollute the real HOME).

## 7. Operational rules and change management
- **Documentation recording rules:** Record changes in `CHANGELOG.md` (English canonical) and `CHANGELOG.ja.md` with version and date. User-facing procedures go in `README.md` / `README.en.md`; release procedures go in `docs/RELEASING.md` / `.en.md`; design changes go in a new ADR under `docs/adr/`. Japanese is the default and the corresponding English version must be updated in the same change (ADR 0005). Convert relative dates to absolute dates.
- **TBD handling:** Mark unknowns as `unverified` and name the primary source to consult next.
- **Release procedure:** Follow `docs/RELEASING.md`. Update the version in `Cargo.toml`, `SOURCE_RELEASE_DATE` in `src/query/version.rs`, and `CHANGELOG.md` / `CHANGELOG.ja.md` to the same version and date → `just check` / `just docs-check` → push to main and get CI green → push annotated tag `vX.Y.Z` → `release.yml` builds Linux x86_64 musl / macOS arm64 / macOS x86_64 and attaches tarballs + `.sha256` to one Release. Never move a published tag.
- **Weekly check:** Use `just weekly-check` to inspect changes since the latest tag. If code, the installer, or the embedded `SKILL.md` changed, classify as patch/minor/major and release. Tests/docs/CI-only changes may be batched into the next code release (ADR 0004).
- **Review / approval flow:** Keep CI (`ci.yml`: fmt-check + clippy(-D warnings) + test + macOS 2-target build + docs pair check) green. Support changes with verification output.
- **Other operational rules:** Generated files (`target/` / `demo-out/` / `dist/`) are not tracked. `install.sh` verifies sha256.

---

### Appendix: reference information
- **Main repositories/directories:** `src/` (CLI), `src/query/` (viewer CLI), `skills/agent-jsonl-compact-reader/` (skill), `.github/workflows/` (CI/Release), `tests/` (fixtures + integration + viewer E2E), `docs/` (ADR / requirements / work record / release procedure / this document), `install.sh`, `scripts/` (weekly check, docs check, benchmark).
- **Representative commands:**
  - `just check` / `just docs-check` / `just weekly-check` / `just smoke-release` / `just demo` / `just dist` / `just bench-viewer`
  - `agent-jsonl-compact -i <session.jsonl> -o <out_dir>`
  - `agent-jsonl-compact --stats` / `agent-jsonl-compact install-skills [--claude-only|--codex-only]`
  - `agent-jsonl-compact sessions --provider codex --cwd <repo>`
  - `agent-jsonl-compact view --session codex:<id> --tail 30 --output jsonl`
  - `agent-jsonl-compact search -i <file> --query <word> --context 2`
- **Dependencies:** `anyhow`, `clap` (derive), `serde`, `serde_json`, `regex`, `sha2`, `base64`, `time` (dev: `tempfile`). See `Cargo.toml` / ADR 0003 for details and MSRV 1.74 pinning rationale.
- **Contact / owner:** git author `yuki-inaho` (yoshikawa@inaho.co). GitHub: `yuki-inaho/agent-jsonl-compact`.

> The template may be expanded or reduced as needed. Keep filled-in documents under version control.

---

### Appendix: session JSONL format cheat sheet

The three JSONL formats are **separate schemas**. Their only common points are roughly "one JSON per line" and "has `type` and `timestamp`".

| | Codex CLI rollout | Claude Code transcript | OpenCode run JSONL |
|---|---|---|---|
| Line shape | `{type, payload, timestamp}` | `{type, message, sessionId, uuid, ...}` | `{type, timestamp, sessionID, part/error}` |
| Body location | `payload` | `message.content` | `part.text` / `part.state` |
| Identifiers | `payload.id` | `sessionId` / `uuid` / `parentUuid` | `sessionID` / `part.messageID` / `part.callID` |
| `type` examples | `session_meta` / `response_item` / `event_msg` | `user` / `assistant` / `summary` | `step_start` / `text` / `reasoning` / `tool_use` / `step_finish` / `error` |
| Characteristics | terminal and api double recording | conversation and tools in one stream | stdout events only; no prompt/model |

- Strong detection signals are `payload` for Codex, `sessionId` for Claude Code, and `sessionID` plus `part/error` for OpenCode.
- Auto-detection samples the first 50 lines (`src/detect.rs`). Only when content is inconclusive does it use the input path (`rollout-*` / `~/.codex/` / `~/.claude/` / `opencode-*`) as a supplementary signal.
- The format-specific implementations are split between `src/classify.rs` (normalization) and `src/clean.rs` (keep set). `--format auto|codex|claude_code|opencode` can override detection explicitly.
- OpenCode normally stores sessions in `~/.local/share/opencode/opencode.db`. The supported JSONL is saved explicitly with `opencode run --format json ... > opencode-session.jsonl`. `opencode export` is a different format.
