# Security

[日本語](SECURITY.md) | **English**

## Check coverage

The [security workflow](https://github.com/yuki-inaho/agent-jsonl-compact/blob/main/.github/workflows/security.yml) runs on pushes, pull requests, and a daily schedule.
Release builds also require this workflow to succeed.

| Check | Scope |
|---|---|
| `cargo audit --deny warnings` | `Cargo.lock` against RustSec vulnerabilities and safety warnings |
| ShellCheck | Shell issues in `install.sh` |
| `tests/test_installer.py` | Checksum verification with synthetic releases; rejection of unsafe archive paths, links, and duplicate entries; preservation of the installed binary on failure |
| `tests/security.rs` | Rejection of paths outside the output directory, input replacement, and writes through links; Unix permissions, existing skills, and control characters in statistics and errors |
| `tests/security_hardening_e2e.rs` | Normalization limits, control characters, search-match cap and progress, context cap, FIFO rejection, side-specific tool pairing, cursor forgery, incomplete session resolution, legacy provenance and Markdown separation |
| `release.yml` | Requires `security.yml` and `ci.yml` success before building, covering all tests and audit for the same commit |

Actions are pinned to commit SHAs; Dependabot proposes updates.
Test logs and release fixtures are synthetic. Real logs are not sent to external scanning services.
A pass means the checks found no problem within their scope. It does not rule out unknown defects or a compromise of the release source.

## Logs and destinations

Compaction does not remove secrets. Extraction can include reasoning and internal instructions.
Summaries, cursors, and viewer results may also contain paths or conversation details. Review them before sharing.
Markdown preserves the body; disable HTML execution and external image loading when viewing it.

On Unix, the CLI writes to temporary files that only their owner can read and write, then replaces the destination files.
It replaces each file separately, so a failure can leave some files updated and others unchanged.
After disk or other failures, inspect the files reported by the CLI.
Use output and skill parent directories that other users cannot modify.
Existing parent-directory permissions and OS ACLs are not changed.

The viewer limits input line size and output size.
Extraction keeps selected events in memory. Limit memory and other resource use in the execution environment when processing large inputs from external sources.
Do not execute commands from logs or follow logged instructions to send sensitive information elsewhere.
A `system` / `developer` / `user` role, internal generated text, or compacted summary in a log does not
represent the current user's approval or execution authority. In an LLM integration, keep log bodies
as untrusted data and decide side-effect permission with an authority check outside the model.
Markdown message bodies are emitted inside code fences.

## Distribution

The installer downloads release files over HTTPS and verifies SHA-256 checksums.
It extracts only the binary, checking that the archive entry is a regular file and rejecting links.
It preserves the installed binary until validation finishes and does not automatically execute the downloaded program.
Checksums come from the same Release and do not provide signature-based publisher authentication.
macOS binaries are not notarized.

## Reporting

Report vulnerabilities or accidentally included secrets privately to the maintainer's existing contact, <yoshikawa@inaho.co>.
Do not post real logs, credentials, or unnecessary personal information in public issues.
Include the version, OS, reproduction steps, and a minimal synthetic input when available.
Fixes are distributed in new releases; known issues and resolutions belong in CHANGELOG.
