# リリース手順

**日本語** | [English](RELEASING.en.md)

バージョン番号の決め方は [ADR 0004](adr/0004-versioning-and-release-policy.md)、日英文書の更新方針は [ADR 0005](adr/0005-bilingual-documentation.md) に従います。
公開済みタグは移動・削除しません。

## 変更を確認する

`just weekly-check` で最新タグと `origin/main` の差分を確認します。
コード・依存関係・インストーラー・埋め込みスキルの変更は配布対象です。
文書・テスト・CIだけの変更は次の配布にまとめられます。
最低対応Rustバージョン（MSRV）の引き上げや、既存CLIの動作変更は、互換性に影響する変更として扱います。

## リリース前

| 更新先 | 内容 |
|---|---|
| `Cargo.toml`, `Cargo.lock` | 同じパッケージ版 |
| `src/query/version.rs` | ソース公開日の固定値 |
| 版・日付を固定するテスト | 新しいmetadataとの一致 |
| `CHANGELOG.md`, `CHANGELOG.ja.md` | 変更内容、互換性に影響する変更（Breaking）、移行方法 |
| README・CLI・スキル等の日英ペア | 現在の手順と仕様。リリース履歴は書き足さない |

```bash
just check
cargo +1.88.0 test --locked
cargo audit --deny warnings
just smoke-release
```

最低対応Rustバージョンは `Cargo.toml` の `rust-version` に定義しています。変更した場合は、上の検査コマンドのバージョンも更新してください。
`cargo audit` は、セキュリティ検査のワークフローで固定しているcargo-auditと同じバージョンを使います。
ローカルの検査が通ったら、対象コミットのCIとセキュリティ検査の結果も確認してください。

公開前にビルドを試すには、`gh workflow run release.yml --ref main` を実行します。
この手動実行（`workflow_dispatch`）では、ビルド成果物をActionsに保存し、Releaseは公開しません。

## タグと配布

対象コミットに注釈付きタグを作り、pushします。`vX.Y.Z` は選んだバージョンに置き換えてください。

```bash
git tag -a vX.Y.Z -m "agent-jsonl-compact X.Y.Z"
git push origin vX.Y.Z
```

`release.yml` はセキュリティ検査の成功後に、Linux x86_64 musl、macOS arm64、macOS x86_64向けのバイナリをビルドします。
各環境で基本動作を確認するsmoke testを行い、すべて成功するとpublishジョブがReleaseに配布物を添付します。
buildジョブの権限は読み取り専用で、publishジョブだけがReleaseへの書き込み権限を持ちます。

| 配布物 | 内容 |
|---|---|
| `agent-jsonl-compact-<target>.tar.gz` | 同名ディレクトリ内のバイナリ、README、LICENSE |
| 同名の `.sha256` | tarballのSHA-256。署名による真正性の保証ではない |

`vX.Y.Z-rc.N` はプレリリースとして公開し、最新安定版にはしません。
GitHubが自動生成するリリースノートは、必要なら `gh release edit vX.Y.Z --notes-file <file>` でCHANGELOGの内容に置き換えます。
互換性に影響する変更と移行方法、対応環境、macOSが未公証であることを記載してください。

## 公開した配布物を検証する

```bash
gh release view vX.Y.Z --json tagName,isDraft,isPrerelease,assets
gh workflow run release-verify.yml -f tag=vX.Y.Z
```

`release-verify.yml` は、Linux、macOS arm64、macOS Intel上で動きます。
公開した配布物を `install.sh` で一時ディレクトリにインストールし、smoke testを実行します。
Releaseを公開したときと、releaseワークフローが完了したときに自動で実行します。
すべての環境で成功したことをActionsで確認してください。ローカルLinuxでの成功だけでは、macOSの動作は確認できません。

不具合があれば、新しい版を公開します。旧Releaseの注意書きは更新できますが、公開済みタグは付け替えません。
GitHub上の現在の状態はActions / Releasesを確認し、本書へ固定して転記しないでください。
