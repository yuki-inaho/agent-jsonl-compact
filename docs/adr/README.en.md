# ADR index

[日本語](README.md) | **English**

Architecture Decision Records (ADRs) document design decisions and their reasons.
Add each new ADR as `NNNN-*.md` (Japanese) and `NNNN-*.en.md` (English); see [ADR 0005](0005-bilingual-documentation.en.md).

| ADR | Topic | Status | Japanese |
|---|---|---|---|
| [0001](0001-additive-session-viewer-cli.md) | Add viewer commands (`sessions` / `view` / `search`) while keeping extraction | Accepted / Implemented | Japanese body |
| [0002](0002-cursor-and-resource-boundaries.md) | Page and append cursors, memory and output limits, and resuming without skipping uninspected records | Accepted / Implemented | Japanese body |
| [0003](0003-version-and-release-metadata.md) | Version, fixed source release date, and rules for tags and releases | Accepted / Implemented | Japanese body |
| [0004](0004-versioning-and-release-policy.en.md) | SemVer application, 0.x handling, patch/minor/major classification, weekly check | Accepted | [Japanese](0004-versioning-and-release-policy.md) |
| [0005](0005-bilingual-documentation.en.md) | Japanese default, `*.en.md`, synchronization and checks | Accepted | [Japanese](0005-bilingual-documentation.md) |
| [0006](0006-safe-installation-and-storage.en.md) | Dependency updates, MSRV, safe storage and installation | Implemented locally | [Japanese](0006-safe-installation-and-storage.md) |

## English summaries of existing ADRs

- **0001** — Add viewer commands (`sessions` / `view` / `search`) alongside extraction.
  Extraction saves files. Viewer commands limit output size, print results with source positions to stdout,
  and leave the input unchanged.
- **0002** — Define page and append cursors and limits on memory and output.
  Treat cursors as opaque values. File replacement, truncation, or an anchor mismatch produces a specific error.
  The viewer does not keep the whole input in memory or skip uninspected records when resuming.
- **0003** — Keep `Cargo.toml` as the semver source, use a fixed `source_release_date`
  (not build time), and never move published tags. Release assets are verified by tests
  and checksums.
