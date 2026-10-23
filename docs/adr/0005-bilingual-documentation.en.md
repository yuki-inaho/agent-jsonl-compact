# ADR 0005 — Bilingual documentation policy

[日本語](0005-bilingual-documentation.md) | **English**

- Status: **Accepted**
- Date: 2026-10-23
- Related: `README.md` / `README.en.md`, `docs/ONBOARDING.md` / `.en.md`,
  `docs/RELEASING.md` / `.en.md`, `docs/README.md` / `.en.md`,
  `docs/adr/README.md` / `.en.md`, `scripts/check-docs.sh`, `.github/workflows/ci.yml`

## 1. Problem

Users and agents of this repository are not all Japanese speakers, but the main
documentation has been written in Japanese. There is a risk that English readers see only
the README and misunderstand defaults or constraints, a risk that Japanese and English
drift apart, and a risk that maintaining full duplicates raises update cost.

Points to decide:

1. Which language is the default
2. Which documents are bilingual
3. File naming and language switcher format
4. Which side is canonical and how to keep them in sync
5. What must not be changed in translation

## 2. Decision

### 2.1 Japanese is the default; English uses `*.en.md`

- Japanese: `README.md`, `docs/ONBOARDING.md`, `docs/RELEASING.md`, `docs/README.md`,
  `docs/adr/README.md`, and new ADRs as `NNNN-*.md`
- English: `README.en.md`, `ONBOARDING.en.md`, `RELEASING.en.md`, `README.en.md`,
  `README.en.md`, and `NNNN-*.en.md` in the same directories

Put a language switcher at the top of `README.md`, with Japanese shown by default. The
English version must link back to Japanese.

### 2.2 Scope

**Living docs (documents users and contributors read continuously) require a
Japanese/English pair.**

- `README.md` / `README.en.md`
- `docs/ONBOARDING.md` / `docs/ONBOARDING.en.md`
- `docs/RELEASING.md` / `docs/RELEASING.en.md`
- `docs/README.md` / `docs/README.en.md` (documentation index)
- `docs/adr/README.md` / `docs/adr/README.en.md` (ADR index)
- Every new ADR (`NNNN-*.md` / `NNNN-*.en.md`)

**Historical records keep Japanese as canonical, with English summaries in the index.**

- `docs/adr/0001`–`0003` (existing ADRs)
- `docs/requirements/session_viewer_cli.md`
- `docs/workdoc_session_jsonl_compactor_rust_port.md`

Full English translations of existing ADRs are not required. `docs/adr/README.en.md`
provides an English summary of each ADR so readers can reach the Japanese body. If a full
translation is added later, follow the `NNNN-*.en.md` naming convention.

### 2.3 Language switcher

Put the following at the top of each pair. The Japanese side emphasizes Japanese and the
English side emphasizes English.

```markdown
**日本語** | [English](README.en.md)
```

```markdown
[日本語](README.md) | **English**
```

In subdirectories use a relative path (e.g. `ONBOARDING.en.md`).

### 2.4 Canonical source and synchronization

- The Japanese version is canonical for user-facing nuance. The English version must not
  omit commands, defaults, constraints, or security warnings.
- Update both languages **in the same commit / PR**. A one-sided change is sent back in
  review.
- If the content conflicts, Japanese wins and English is corrected. Executable contracts
  such as commands and JSON keys must be identical on both sides.
- Translation need not be literal. Concision for readability is allowed, but warnings,
  defaults, and "do not do this" guidance must not be dropped.

### 2.5 What is not translated

Code identifiers, CLI flags, subcommand names, JSON keys, exit codes, file paths, URLs,
environment variables, schema versions, `source_release_date`, and other contract values
stay in their original form.

### 2.6 CHANGELOG exception

`CHANGELOG.md` is English canonical and `CHANGELOG.ja.md` is its Japanese counterpart.
This matches the existing test (`a11_version_consistency` reads `CHANGELOG.md`) and common
changelog tooling conventions. Both must contain the same version and date.

### 2.7 Checks

`scripts/check-docs.sh` checks:

- the Japanese/English pair exists for each target
- each file contains a link to the counterpart filename

Run it via `just docs-check` and in CI (`.github/workflows/ci.yml`). The release procedure
(`docs/RELEASING.md`) includes `just docs-check` as a required step.

## 3. Alternatives considered

| Alternative | Advantage | Why not chosen |
|---|---|---|
| Make English the default | Immediately accessible to international users | Less natural for Japanese-speaking maintainers/users; existing assets are Japanese |
| Put Japanese and English in one document | No extra files | Documents become long, no real language switcher, and diffs are hard to read |
| Machine translation only | Low cost | Cannot detect drift in defaults, warnings, or contracts; canonical source becomes ambiguous |
| Separate English repository | Can separate audiences | Adds synchronization cost and link-rot risk |

## 4. Consequences

- Pro: Japanese speakers keep Japanese as the default, and English speakers can reach the same content.
- Pro: The switcher and checks detect one-sided updates.
- Pro: Historical records do not require full translation, keeping maintenance cost down.
- Con: Every living-doc update touches two files.
- Con: English nuance may be weaker than Japanese. Japanese wins on conflict.
- Con: Existing ADRs 0001–0003, requirements, and the work record are not fully translated.

## 5. Verification

| Check | Method |
|---|---|
| Pair existence | `scripts/check-docs.sh` |
| Cross-links | `scripts/check-docs.sh` |
| CI enforcement | `Docs language pairs` in `.github/workflows/ci.yml` |
| Release check | Checklist in `docs/RELEASING.md` |

## 6. Limitations

- The check verifies file existence and links only; it does not verify semantic equivalence.
- The English version can still fall behind. Review and the release checklist mitigate this.
- English summaries of existing ADRs are key points, not full translations.
