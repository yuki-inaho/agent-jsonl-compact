# ADR 索引

**日本語** | [English](README.en.md)

設計判断とその理由をADR（Architecture Decision Record）として記録しています。新しいADRは `NNNN-*.md`（日本語）と
`NNNN-*.en.md`（英語）のペアで追加します（[ADR 0005](0005-bilingual-documentation.md)）。

| ADR | 主題 | 状態 | 英語 |
|---|---|---|---|
| [0001](0001-additive-session-viewer-cli.md) | 抽出機能を維持し、閲覧CLI（`sessions` / `view` / `search`）を追加 | Accepted / Implemented | 索引に要約 |
| [0002](0002-cursor-and-resource-boundaries.md) | ページ・追記用カーソル、メモリと出力の上限、未検査の行を飛ばさない再開処理 | Accepted / Implemented | 索引に要約 |
| [0003](0003-version-and-release-metadata.md) | バージョンと固定のソース公開日、タグとリリースの管理規則 | Accepted / Implemented | 索引に要約 |
| [0004](0004-versioning-and-release-policy.md) | SemVer の適用、0.x の扱い、patch/minor/major の分類、週次チェック | Accepted | [English](0004-versioning-and-release-policy.en.md) |
| [0005](0005-bilingual-documentation.md) | 日本語デフォルト、`*.en.md`、同期と検査 | Accepted | [English](0005-bilingual-documentation.en.md) |
| [0006](0006-safe-installation-and-storage.md) | 依存関係の更新、MSRV、安全な保存と導入 | Implemented locally | [English](0006-safe-installation-and-storage.en.md) |
| [0007](0007-untrusted-log-evidence.md) | 非信頼ログの証拠化、正規化・対応付け・cursor・探索・表示の資源境界 | Implemented locally | [English](0007-untrusted-log-evidence.en.md) |

## 既存ADRの英語要約

- **0001** — Add viewer commands (`sessions` / `view` / `search`) alongside extraction.
  Extraction saves files. Viewer commands limit output size, print results with source positions to stdout,
  and leave the input unchanged.
- **0002** — Define page and append cursors and limits on memory and output.
  Treat cursors as opaque values. File replacement, truncation, or an anchor mismatch produces a specific error.
  The viewer does not keep the whole input in memory or skip uninspected records when resuming.
- **0003** — Keep `Cargo.toml` as the semver source, use a fixed `source_release_date`
  (not build time), and never move published tags. Release assets are verified by tests
  and checksums.
