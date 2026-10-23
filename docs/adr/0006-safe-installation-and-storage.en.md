# ADR 0006 — Dependency updates and safe storage / installation

[日本語](0006-safe-installation-and-storage.md) | **English**

- Status: Implemented locally; remote workflows not yet run for this change
- Date: 2026-10-23
- Related: [ADR 0003 §2.4](0003-version-and-release-metadata.md), [ADR 0004](0004-versioning-and-release-policy.en.md)

## Problem and evidence

A review of setup and security found that `cargo-audit 0.22.2` with `cargo audit --deny warnings`
failed on `time 0.3.36` ([RUSTSEC-2026-0009](https://rustsec.org/advisories/RUSTSEC-2026-0009))
and `anyhow 1.0.102` ([RUSTSEC-2026-0190](https://rustsec.org/advisories/RUSTSEC-2026-0190)).
The affected APIs are RFC 2822 parsing and `Error::downcast_mut`; the current code does not call them.
ADR 0003 had pinned `time` to preserve Rust 1.74. Patched `time 0.3.47` requires Rust 1.88 (`cargo info time@0.3.47`).

Direct output creation allowed name traversal, writes through destination links, input replacement,
and broadly readable Unix exports. Skill installation unconditionally overwrote user edits.
The installer extracted the whole archive and selected the first matching binary name.

## Decision

Raise MSRV to 1.88, require `time >=0.3.47` and patched `anyhow >=1.0.104`, update the lockfile,
and test it on MSRV with `cargo test --locked` in CI. This supersedes ADR 0003 §2.4's MSRV decision.
Version, source-date, and schema contracts are unchanged.

Restrict output names to a single filename without separators or control characters.
Check all destinations before writing; reject input aliases and non-regular destinations.
Promote `tempfile` to a runtime dependency and replace outputs through same-directory temporary files.
Saved Unix files are 0600; other names hardlinked to old outputs remain unchanged.
Escape control characters in terminal statistics, paths, and errors without changing saved bodies.

Keep existing skills unless `--force` is explicit. Reject symlinked skill directories and `SKILL.md` files even with force.
Validate release tags, checksum records, archive names, and the regular-file binary member.
Stream only that member, then replace the installed binary through a staged file.
Do not automatically run downloaded binaries or advise unconditional quarantine removal.
Explain that SHA-256 is not signature-based authentication.

Run audit warnings, ShellCheck, and storage/installer regressions in a read-only security workflow required by release builds.
Pin Actions to commit SHAs. Honor the user's instruction to omit a security badge from README.

## Alternatives

| Option | Assessment |
|---|---|
| Preserve Rust 1.74 and exempt advisories based on unreachable APIs | Rejected: creates continuing exemption maintenance |
| Replace or implement date parsing to retain MSRV | Rejected for this change: requires broader date-boundary compatibility work |
| Update dependencies and raise MSRV | Chosen: changes source-build requirements; prebuilt users still need no Rust |
| Reject all existing output files | Rejected: retain re-extraction through safe file replacement |

## Verification and limitations

`tests/security.rs` covers names, input aliases, links, permissions, existing skills, and force.
`tests/test_installer.py` uses real tar/checksum tools with synthetic network responses.
Full MSRV Rust tests, installer tests, and the updated dependency audit passed locally on Linux.
Native macOS validation and remote Actions for this change have not run.

Publisher authentication, unknown vulnerabilities, hostile shared parent directories, and multi-output rollback are outside these guarantees.
The next release must describe the MSRV increase, preserved existing skills, and destination restrictions as Breaking changes.
