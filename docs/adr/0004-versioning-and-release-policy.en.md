# ADR 0004 — Versioning and release policy

[日本語](0004-versioning-and-release-policy.md) | **English**

- Status: **Accepted**
- Date: 2026-10-23
- Related: [ADR 0003](0003-version-and-release-metadata.md), `Cargo.toml`, `src/query/version.rs`,
  `CHANGELOG.md`, `docs/RELEASING.md`, `.github/workflows/release.yml`

## 1. Problem

0.3.0 introduced query schema v1 and cursor v1 with the viewer CLI, and ADR 0003 decided
that semver lives in `Cargo.toml`, the update date is a fixed source date, and published
tags are never moved. However, the following were not written down:

1. Which changes count as major / minor / patch
2. How much of the compatibility contract is protected during 0.x
3. How to decide weekly whether an update exists, and when to release
4. How to version a change that only adds distribution artifacts (Linux / macOS)
5. How to handle hotfixes and pre-releases

Without a policy, docs-only changes can trigger unnecessary releases, while a change to
the embedded `SKILL.md` can accidentally be left out of the distribution. The decision
needs to be reproducible.

## 2. Decision

### 2.1 Adopt SemVer 2.0.0

Versions are `MAJOR.MINOR.PATCH`, and `version` in `Cargo.toml` is the single source of
truth. `src/query/version.rs` reads `env!("CARGO_PKG_VERSION")` and never hardcodes a
version string.

### 2.2 Handling 0.x

SemVer allows arbitrary changes in 0.y.z, but this project adds the following rule to
protect users:

- A **patch** release in `0.y.z` is backward compatible. It does not change CLI behavior,
  output schema, cursors, or exit codes.
- A **minor** release `0.y+1.0` may contain breaking changes. If it does, CHANGELOG must
  include a `Breaking` section and migration guidance.
- From 1.0.0 onward, normal SemVer applies: breaking changes only in a major release.

### 2.3 Change classification

| Kind of change | Version | Examples |
|---|---|---|
| Breaking the compatibility contract | major (1.x+) / minor (0.x) | Removing or renaming a CLI flag, removing or renaming a JSON field, bumping query schema / cursor version, changing exit-code contracts, changing `install-skills` destinations |
| Backward-compatible addition | minor | New subcommand, new option, new JSON field, new provider support, new platform support that requires code changes |
| Backward-compatible fix / distribution | patch | Bug fix, docs, CI, installer fix, new prebuilt architecture without a code-contract change, embedded `SKILL.md` update |
| No release required (may be batched) | none | Typo in non-embedded docs, tests/CI-only additions, internal refactor with no user-visible change |

Examples:

- `0.3.0`: minor. Added `sessions` / `view` / `search` while keeping the existing
  extraction schema, defaults, and exit codes.
- `0.3.1`: patch. Added macOS prebuilt binaries, the release matrix, ADR 0004/0005, and
  bilingual documentation. Query schema / cursor / CLI behavior are unchanged.

Explicit compatibility contracts:

- legacy extraction flags, defaults, output schema, and exit codes
- viewer query schema v1, cursor v1, JSON keys, and exit codes `0` / `1` / `2`
- `install-skills` destinations
- release asset names and `.sha256` format
- MSRV (raising it is a minor change and must be noted in CHANGELOG)

### 2.4 Release need and weekly check

Every week, compare the latest `v*` tag with `origin/main`. The reproducible command is
`just weekly-check` (`scripts/weekly-release-check.sh`).

- No diff: do not release.
- Changes under `src/`, `Cargo.toml`, `Cargo.lock`, `skills/`, or `install.sh`:
  classify per 2.3 and release. In particular,
  `skills/agent-jsonl-compact-reader/SKILL.md` is embedded into the binary, so a change
  that looks like docs still changes the distribution.
- Only tests / docs / CI / metadata: normally batch into the next code release. Test and
  documentation additions alone do not trigger a release. Cut a patch if the change fixes
  distribution or the installer.
- If more than 7 days have passed since the latest tag and release-relevant changes
  exist, release promptly.

A release requires all of the following:

1. `just check` is green (it includes `docs-check`)
2. Linux x86_64 musl / macOS arm64 / macOS x86_64 assets build
3. Each asset has a `.sha256` and is attached to the Release
4. `CHANGELOG.md` and `CHANGELOG.ja.md` contain the same version and date

### 2.5 Where the version is updated

The release commit updates all of the following together:

- `version` in `Cargo.toml` (`Cargo.lock` follows)
- `SOURCE_RELEASE_DATE` in `src/query/version.rs`
- `CHANGELOG.md` / `CHANGELOG.ja.md`
- tests that pin the version/date (e.g. `a11_version_consistency` in
  `tests/viewer_e2e.rs`)

### 2.6 Tags, pre-releases, and hotfixes

- A release is an annotated tag `vX.Y.Z` on a merged commit on main.
- **Published tags are never moved or deleted.** On a defect, cut a new patch and, if
  necessary, add a "do not use" note to the old Release.
- A pre-release is `vX.Y.Z-rc.N`, published as a GitHub prerelease and not marked latest.
  General users are directed to stable releases.
- A hotfix branches from the latest tag, bumps patch, and merges back to main.

### 2.7 `source_release_date`

`SOURCE_RELEASE_DATE` is the date the source was frozen as a release. It is not the build
time or the GitHub Release publication time. It is updated in the release commit, and the
same source always returns the same value. This inherits the decision in ADR 0003.

## 3. Alternatives considered

| Alternative | Advantage | Why not chosen |
|---|---|---|
| CalVer (date-based) | Release date is obvious | Cannot express compatibility size; users cannot judge upgrades |
| Use commit count or build number as the version | Easy to automate | Carries no compatibility information and conflicts with the SemVer contract |
| Always release even for docs changes | Distribution is always current | Produces near-empty Releases and distracts upgrade decisions |
| Put all breaking changes into patches during 0.x | Version number stays small | Breaks the backward-compatibility contract of patches and makes upgrades unsafe |

## 4. Consequences

- Pro: The version number communicates the size of the compatibility change.
- Pro: The weekly check and release decision are reproducible with a script.
- Pro: Embedded skill changes cannot be forgotten in a release.
- Con: Each release requires updating several places (Cargo / version.rs / CHANGELOG / tests).
- Con: During 0.x, a minor release may be breaking, so users must read `Breaking` in CHANGELOG.

## 5. Verification

| Check | Method |
|---|---|
| Version consistency | `tests/viewer_e2e.rs::a11_version_consistency` |
| Fixed date | unit test in `src/query/version.rs` |
| Weekly decision | `just weekly-check` (read-only) |
| Release assets | matrix build in `release.yml` and Release attachment |
| Japanese/English docs sync | `just docs-check` |

## 6. Limitations

- This ADR does not guarantee the outcome of any individual release. See GitHub Actions /
  Releases for current state.
- Classification requires judgment. When in doubt, use "would this break an existing user
  script?" as the criterion, and treat breaking changes as major (minor during 0.x).
- The weekly check is currently manual; scheduled execution is not configured.
