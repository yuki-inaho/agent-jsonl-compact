# Release procedure

[日本語](RELEASING.md) | **English**

Follow [ADR 0004](adr/0004-versioning-and-release-policy.en.md) to choose version numbers and [ADR 0005](adr/0005-bilingual-documentation.en.md) to update Japanese and English documentation together.
Never move or delete published tags.

## Review changes

Use `just weekly-check` to compare the latest tag with `origin/main`.
Changes to code, dependencies, the installer, or the embedded skill require a release to reach users.
Documentation, tests, and CI-only changes may wait for the next release.
Treat increases in the minimum supported Rust version (MSRV) and changes to existing CLI behavior as compatibility changes.

## Before release

| Update | Content |
|---|---|
| `Cargo.toml`, `Cargo.lock` | Matching package version |
| `src/query/version.rs` | Fixed source release date |
| Tests with fixed version/date | Match the new metadata |
| `CHANGELOG.md`, `CHANGELOG.ja.md` | Changes, breaking changes, and migration steps |
| README, CLI, skill, and other language pairs | Current procedures and behavior; keep release history in CHANGELOG |

```bash
just check
cargo +1.88.0 test --locked
cargo audit --deny warnings
just smoke-release
```

The `rust-version` field in `Cargo.toml` defines the minimum supported Rust version. Update the test command above when it changes.
Use the cargo-audit version pinned in the security workflow.
After local checks, inspect CI and security results for the target commit.

To test the build before publishing, run `gh workflow run release.yml --ref main`.
This manual run (`workflow_dispatch`) saves build artifacts in Actions without publishing a Release.

## Tag and distribute

Create and push an annotated tag on the target commit. Replace `vX.Y.Z` with the chosen version.

```bash
git tag -a vX.Y.Z -m "agent-jsonl-compact X.Y.Z"
git push origin vX.Y.Z
```

After security checks succeed, `release.yml` builds Linux x86_64 musl, macOS arm64, and macOS x86_64.
Smoke tests check basic behavior on each target. Publication requires all builds to succeed.
Build jobs have read permission; only the publish job can write Release assets.

| Asset | Contents |
|---|---|
| `agent-jsonl-compact-<target>.tar.gz` | Binary, README, and LICENSE inside the matching directory |
| Matching `.sha256` | Tarball SHA-256; not signature-based authenticity |

Tags such as `vX.Y.Z-rc.N` publish prereleases and do not become the latest stable release.
Use `gh release edit vX.Y.Z --notes-file <file>` to replace generated notes with CHANGELOG content when needed.
Include breaking changes, migration steps, supported targets, and the lack of macOS notarization.

## Verify published assets

```bash
gh release view vX.Y.Z --json tagName,isDraft,isPrerelease,assets
gh workflow run release-verify.yml -f tag=vX.Y.Z
```

`release-verify.yml` installs published assets through `install.sh` into a temporary directory and runs smoke tests on native Linux, macOS arm64, and macOS Intel.
It runs automatically after Release publication or completion of the release workflow.
Confirm that every target succeeds in Actions. A local Linux pass does not verify macOS execution.

Publish a new version for fixes. You may update warnings on an old Release, but never move its tag.
Check Actions / Releases for current remote state rather than copying it into this guide.
