# リリース手順

**日本語** | [English](RELEASING.en.md)

版の分類は [ADR 0004](adr/0004-versioning-and-release-policy.md)、文書の同期は [ADR 0005](adr/0005-bilingual-documentation.md) に従います。
公開済みタグは移動・削除しません。

## 変更を確認する

`just weekly-check` で最新タグと `origin/main` の差分を確認します。
コード・依存関係・インストーラー・埋め込みスキルの変更は配布対象です。
文書・テスト・CIだけの変更は次の配布にまとめられます。
MSRVの引上げや既存CLIの動作変更は、互換性に影響する変更として分類してください。

## リリース前

| 更新先 | 内容 |
|---|---|
| `Cargo.toml`, `Cargo.lock` | 同じパッケージ版 |
| `src/query/version.rs` | 固定のsource release date |
| 版・日付を固定するテスト | 新しいmetadataとの一致 |
| `CHANGELOG.md`, `CHANGELOG.ja.md` | 変更、Breaking、移行方法 |
| README・CLI・スキル等の日英ペア | 現在の手順と仕様。リリース履歴は書き足さない |

```bash
just check
cargo +1.88.0 test --locked
cargo audit --deny warnings
just smoke-release
```

MSRVは `Cargo.toml` が正本です。上のtoolchainはMSRVを変えた場合に追従してください。
`cargo audit` はsecurity workflowと同じ固定版のcargo-auditを使用します。
ローカルの合格後、対象commitのCIとsecurity workflowも確認してください。

手動の配布検証は `gh workflow run release.yml --ref main` で行えます。
`workflow_dispatch` ではbuild artifactを残し、Releaseは公開しません。

## タグと配布

対象commitへannotated tagを作り、pushします。`vX.Y.Z` は選んだ版へ置き換えてください。

```bash
git tag -a vX.Y.Z -m "agent-jsonl-compact X.Y.Z"
git push origin vX.Y.Z
```

`release.yml` はsecurity workflowの成功後に、Linux x86_64 musl・macOS arm64・macOS x86_64をbuildします。
各targetでsmoke testを行い、すべて成功した場合だけpublish jobがReleaseへ添付します。
build jobは読み取り権限、publish jobだけがReleaseの書き込み権限を持ちます。

| 資産 | 内容 |
|---|---|
| `agent-jsonl-compact-<target>.tar.gz` | 同名ディレクトリ内のバイナリ、README、LICENSE |
| 同名の `.sha256` | tarballのSHA-256。署名による真正性の保証ではない |

`vX.Y.Z-rc.N` はprereleaseとして公開し、最新安定版にはしません。
GitHubの自動生成notesは、必要なら `gh release edit vX.Y.Z --notes-file <file>` でCHANGELOGの内容に置き換えます。
Breakingと移行方法、対応target、macOSが未公証であることを記載してください。

## 公開した資産を検証する

```bash
gh release view vX.Y.Z --json tagName,isDraft,isPrerelease,assets
gh workflow run release-verify.yml -f tag=vX.Y.Z
```

`release-verify.yml` はnativeのLinux / macOS arm64 / macOS Intelで、公開資産を `install.sh` 経由で一時ディレクトリへ導入し、smoke testを実行します。
Release公開イベントとrelease workflowの完了が自動実行の契機です。
全targetの成功をActionsで確認してください。ローカルLinuxの成功だけでmacOSも検証済みとは扱いません。

不具合があれば、新しい版を公開します。旧Releaseの注意書きは更新できますが、公開済みタグは付け替えません。
GitHub上の現在の状態はActions / Releasesを確認し、本書へ固定して転記しないでください。
