# ドキュメント索引

**日本語** | [English](README.en.md)

| ドキュメント | 日本語 | English | 内容 |
|---|---|---|---|
| プロジェクト概要 | [README.md](../README.md) | [README.en.md](../README.en.md) | インストール、使い方、閲覧 CLI、開発レイアウト |
| LLM オンボーディング | [ONBOARDING.md](ONBOARDING.md) | [ONBOARDING.en.md](ONBOARDING.en.md) | 前提、制約、参照資料、試行タスク、運用ルール |
| リリース手順 | [RELEASING.md](RELEASING.md) | [RELEASING.en.md](RELEASING.en.md) | 週次チェック、版の決め方、tag / Release、資産検証 |
| 変更履歴 | [CHANGELOG.ja.md](../CHANGELOG.ja.md) | [CHANGELOG.md](../CHANGELOG.md) | 版ごとの変更点（英語が正本） |
| ADR 索引 | [adr/README.md](adr/README.md) | [adr/README.en.md](adr/README.en.md) | 設計判断の一覧と英語サマリ |
| バージョニング基準 | [adr/0004-versioning-and-release-policy.md](adr/0004-versioning-and-release-policy.md) | [adr/0004-versioning-and-release-policy.en.md](adr/0004-versioning-and-release-policy.en.md) | SemVer、0.x、patch/minor/major の分類 |
| 二言語ドキュメント方針 | [adr/0005-bilingual-documentation.md](adr/0005-bilingual-documentation.md) | [adr/0005-bilingual-documentation.en.md](adr/0005-bilingual-documentation.en.md) | 日本語デフォルト、`*.en.md`、同期と検査 |
| 閲覧 CLI 要件 | [requirements/session_viewer_cli.md](requirements/session_viewer_cli.md) | — | F01–F10 / N01–N05 と受入ケース（日本語のみ） |
| 移植作業記録 | [workdoc_session_jsonl_compactor_rust_port.md](workdoc_session_jsonl_compactor_rust_port.md) | — | 移植根拠、モジュール対応、DOD（日本語のみ） |

## 言語ルール

- 利用者・参加者が継続的に読む文書は日本語と英語のペアで維持します。
- 日本語をデフォルトとし、英語版は `*.en.md` に置きます。
- 既存 ADR 0001–0003、要件、作業記録は日本語を正本とし、英語では ADR 索引に要約を置きます。
- 詳細は [ADR 0005](adr/0005-bilingual-documentation.md) を参照してください。
