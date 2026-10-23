# Security

[日本語](SECURITY.md) | **English**

## Check coverage

The [security workflow](.github/workflows/security.yml) runs on pushes, pull requests, and a daily schedule.
Release builds also require this workflow to succeed.

| Check | Scope |
|---|---|
| `cargo audit --deny warnings` | `Cargo.lock` against RustSec vulnerabilities and safety warnings |
| ShellCheck | Shell issues in `install.sh` |
| `tests/test_installer.py` | Synthetic checksums, archive traversal, links, duplicates, and preserving the existing binary on failure |
| `tests/security.rs` | Output traversal, input replacement, link targets, Unix permissions, existing skills, and control characters in statistics/errors |

Actions are pinned to commit SHAs; Dependabot proposes updates.
Test logs and release fixtures are synthetic. Real logs are not sent to external scanning services.
Passing means these checks found no problem; it does not establish the absence of unknown defects or publisher compromise.

## Logs and destinations

Compaction does not remove secrets. Extraction can include reasoning and internal instructions.
Summaries, cursors, and viewer results may also contain paths or conversation details. Review them before sharing.
Markdown preserves the body; disable HTML execution and external image loading when viewing it.

Saved Unix files are owner-only and replace their destinations through temporary files.
Multiple outputs are not a single transaction. After disk or other failures, inspect the files reported by the CLI.
Use output and skill parent directories that other users cannot modify.
Existing parent-directory permissions and OS ACLs are not changed.

The viewer has record and output limits. Extraction retains accepted events in memory, so apply resource limits when processing large inputs from external sources.
Do not execute commands from logs or follow logged instructions to send sensitive information elsewhere.

## Distribution

The installer uses HTTPS and SHA-256 verification, then streams only the archive's regular binary member.
It preserves the installed binary until validation finishes and does not automatically execute the downloaded program.
Checksums come from the same Release and do not provide signature-based publisher authentication.
macOS binaries are not notarized.

## Reporting

Report vulnerabilities or accidentally included secrets privately to the maintainer's existing contact, <yoshikawa@inaho.co>.
Do not post real logs, credentials, or unnecessary personal information in public issues.
Include the version, OS, reproduction steps, and a minimal synthetic input when available.
Fixes are distributed in new releases; known issues and resolutions belong in CHANGELOG.
