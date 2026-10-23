# Releasing and versioning

[日本語](RELEASING.md) | **English**

Version classification follows [ADR 0004](adr/0004-versioning-and-release-policy.en.md);
Japanese/English documentation synchronization follows
[ADR 0005](adr/0005-bilingual-documentation.en.md). This document is the execution
procedure.

## 1. Weekly check

Compare the latest `v*` tag with `origin/main` and decide whether a release is needed.

```bash
just weekly-check
```

Internally, `scripts/weekly-release-check.sh` prints:

- the latest tag and its age in days
- commits since the latest tag
- changed paths
- whether release-relevant paths changed (`src/`, `tests/`, `Cargo.toml`, `Cargo.lock`,
  `skills/`, `install.sh`)

Decision:

| Situation | Action |
|---|---|
| No diff | Do not release |
| Code / tests / installer / embedded `SKILL.md` changed | Classify patch / minor / major and release |
| Only docs / CI / metadata changed | Batch into the next code release; cut a patch if distribution changed |
| More than 7 days since the latest tag and release-relevant changes exist | Release promptly |

`skills/agent-jsonl-compact-reader/SKILL.md` is embedded into the binary, so a change that
looks like docs still changes the distribution. Always treat it as release-relevant.

## 2. Choosing the version

| Change | Version |
|---|---|
| Breaking the compatibility contract (removed flag, changed JSON key, schema / cursor bump, changed exit code) | major (1.x+) / minor (0.x) |
| Backward-compatible addition (new subcommand, option, field, provider) | minor |
| Backward-compatible fix / distribution (bug fix, docs, CI, installer, new prebuilt arch, embedded skill) | patch |
| No user-visible change | No release required |

When in doubt, use "would this break an existing user script?" as the criterion. If it
would, treat it as major (minor during 0.x).

## 3. Pre-release checklist

Update all of the following in the release commit:

- [ ] `version` in `Cargo.toml`
- [ ] `Cargo.lock` (e.g. `cargo update -p agent-jsonl-compact --offline`)
- [ ] `SOURCE_RELEASE_DATE` in `src/query/version.rs`
- [ ] tests that pin the version/date (e.g. `tests/viewer_e2e.rs::a11_version_consistency`)
- [ ] `CHANGELOG.md` (English canonical) and `CHANGELOG.ja.md`
- [ ] affected README / ONBOARDING / RELEASING / ADR Japanese/English pairs

Verify:

```bash
just check        # fmt-check + clippy -D warnings + test + docs-check
just docs-check   # run only the Japanese/English pair check
```

Push to main and confirm CI (Linux check + macOS 2-target build + docs pair check) is
green. Before tagging, you can dry-run the release workflow manually
(`workflow_dispatch` skips the publish job and leaves build artifacts only):

```bash
gh workflow run release.yml --ref main
gh run list --workflow release.yml --limit 3
```

## 4. Tag and Release

After CI is green, create an annotated tag on the target commit on main and push it:

```bash
git tag -a vX.Y.Z -m "agent-jsonl-compact X.Y.Z"
git push origin vX.Y.Z
```

`release.yml` builds the following three targets in a matrix and attaches them to one
Release:

- `x86_64-unknown-linux-musl`
- `aarch64-apple-darwin`
- `x86_64-apple-darwin`

Each target produces `agent-jsonl-compact-<target>.tar.gz` and `.sha256`. The publish job
runs only after every build job succeeds.

## 5. Asset verification

```bash
gh release view vX.Y.Z --json tagName,isDraft,isPrerelease,assets
```

Expected assets:

```text
agent-jsonl-compact-x86_64-unknown-linux-musl.tar.gz (+ .sha256)
agent-jsonl-compact-aarch64-apple-darwin.tar.gz      (+ .sha256)
agent-jsonl-compact-x86_64-apple-darwin.tar.gz       (+ .sha256)
```

Download and verify checksums and contents:

```bash
tmp="$(mktemp -d)"
gh release download vX.Y.Z --dir "$tmp"
( cd "$tmp" && sha256sum -c ./*.sha256 )
for a in "$tmp"/*.tar.gz; do tar -C "$tmp" -xzf "$a"; done
file "$tmp"/agent-jsonl-compact-*/agent-jsonl-compact
"$tmp/agent-jsonl-compact-x86_64-unknown-linux-musl/agent-jsonl-compact" --version
```

macOS binaries cannot run on Linux. Check the Mach-O arch with `file` and, when possible,
verify `--version` and `version --output json` on a real macOS machine. macOS binaries are
ad-hoc signed only and not notarized. If Gatekeeper quarantines one, advise
`xattr -d com.apple.quarantine <binary>`.

## 6. Release notes

Replace the auto-generated release body with the relevant CHANGELOG section if needed:

```bash
gh release edit vX.Y.Z --notes-file temp/release-notes-vX.Y.Z.md
```

Include at least:

- version and fixed source release date
- main changes (`Breaking` called out explicitly)
- attached assets (three targets and checksums)
- macOS signing / notarization caveat

## 7. Hotfix and defects

- Never move or delete a published tag.
- For a defect, branch from the latest tag, bump patch, fix, merge back to main, and create
  a new tag.
- To mark an old Release as unusable, use `gh release edit`.

## 8. Pre-release

A tag of the form `vX.Y.Z-rc.N` is published as a prerelease by `release.yml` and is not
marked latest. Direct general users to stable releases.

## 9. Limitations

- The weekly check is currently manual; scheduled execution is not configured.
- CI verifies macOS builds, but execution tests are mainly the Linux binary on Linux.
- Do not freeze the current Release state in this document; check GitHub Actions / Releases.
