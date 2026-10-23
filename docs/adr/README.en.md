# ADR index

[日本語](README.md) | **English**

Architecture Decision Records. New ADRs are added as a pair of `NNNN-*.md` (Japanese) and
`NNNN-*.en.md` (English) — see [ADR 0005](0005-bilingual-documentation.en.md).

| ADR | Topic | Status | Japanese |
|---|---|---|---|
| [0001](0001-additive-session-viewer-cli.md) | Add an additive viewer CLI (`sessions` / `view` / `search`) without replacing extraction | Accepted / Implemented | Japanese body |
| [0002](0002-cursor-and-resource-boundaries.md) | Page / append cursors and resource boundaries (do not retain everything; never advance silently) | Accepted / Implemented | Japanese body |
| [0003](0003-version-and-release-metadata.md) | Version and fixed source release date; tag / release contract | Accepted / Implemented | Japanese body |
| [0004](0004-versioning-and-release-policy.en.md) | SemVer application, 0.x handling, patch/minor/major classification, weekly check | Accepted | [English](0004-versioning-and-release-policy.en.md) |
| [0005](0005-bilingual-documentation.en.md) | Japanese default, `*.en.md`, synchronization and checks | Accepted | [English](0005-bilingual-documentation.en.md) |

## English summaries of existing ADRs

- **0001** — Add the viewer CLI (`sessions` / `view` / `search`) as an additive layer
  alongside legacy extraction. Extraction still saves files; viewer commands return
  bounded results to stdout with provenance and no source mutation.
- **0002** — Define page / append cursors and resource boundaries. Cursors are opaque,
  typed, and fail closed on generation/truncation/anchor mismatches; the CLI does not
  keep the whole input or silently advance past uninspected records.
- **0003** — Keep `Cargo.toml` as the semver source, use a fixed `source_release_date`
  (not build time), and never move published tags. Release assets are verified by tests
  and checksums.
