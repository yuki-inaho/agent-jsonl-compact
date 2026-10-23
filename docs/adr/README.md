# ADR 索引

**日本語** | [English](README.en.md)

設計判断の記録（Architecture Decision Records）。新しい ADR は `NNNN-*.md`（日本語）と
`NNNN-*.en.md`（英語）のペアで追加します（[ADR 0005](0005-bilingual-documentation.md)）。

| ADR | 主題 | 状態 | 英語 |
|---|---|---|---|
| [0001](0001-additive-session-viewer-cli.md) | 既存抽出を置き換えない閲覧 CLI（`sessions` / `view` / `search`）の追加 | Accepted / Implemented | 索引にサマリ |
| [0002](0002-cursor-and-resource-boundaries.md) | page / append cursor と資源境界（保持しない・暗黙に進めない） | Accepted / Implemented | 索引にサマリ |
| [0003](0003-version-and-release-metadata.md) | 版と固定 source release date、tag / release の契約 | Accepted / Implemented | 索引にサマリ |
| [0004](0004-versioning-and-release-policy.md) | SemVer の適用、0.x の扱い、patch/minor/major の分類、週次チェック | Accepted | [English](0004-versioning-and-release-policy.en.md) |
| [0005](0005-bilingual-documentation.md) | 日本語デフォルト、`*.en.md`、同期と検査 | Accepted | [English](0005-bilingual-documentation.en.md) |
| [0006](0006-safe-installation-and-storage.md) | 依存関係の更新、MSRV、安全な保存と導入 | Implemented locally | [English](0006-safe-installation-and-storage.en.md) |

## 既存 ADR の英語サマリ

- **0001** — Add the viewer CLI (`sessions` / `view` / `search`) as an additive layer
  alongside legacy extraction. Extraction still saves files; viewer commands return
  bounded results to stdout with provenance and no source mutation.
- **0002** — Define page / append cursors and resource boundaries. Cursors are opaque,
  typed, and fail closed on generation/truncation/anchor mismatches; the CLI does not
  keep the whole input or silently advance past uninspected records.
- **0003** — Keep `Cargo.toml` as the semver source, use a fixed `source_release_date`
  (not build time), and never move published tags. Release assets are verified by tests
  and checksums.
