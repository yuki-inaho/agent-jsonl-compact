# リリースとバージョニング

**日本語** | [English](RELEASING.en.md)

版の分類は [ADR 0004](adr/0004-versioning-and-release-policy.md)、日英ドキュメントの
同期は [ADR 0005](adr/0005-bilingual-documentation.md) に従います。本書は実行手順です。

## 1. 週次チェック

最新の `v*` タグと `origin/main` を比較し、リリースが必要かを確認します。

```bash
just weekly-check
```

内部では `scripts/weekly-release-check.sh` が次を表示します。

- 最新タグ、タグの経過日数
- 最新タグ以降の commit 一覧
- 変更されたパス
- リリース対象の変更（`src/`、`tests/`、`Cargo.toml`、`Cargo.lock`、`skills/`、
  `install.sh`）の有無

判定:

| 状況 | 対応 |
|---|---|
| 差分 0 件 | リリースしない |
| コード・installer・埋め込み `SKILL.md` に変更 | patch / minor / major を分類してリリース |
| tests / docs / CI / metadata のみ | 次回コードリリースへまとめる。リリース不要 |
| 最終タグから 7 日以上 + リリース対象あり | 速やかにリリース |

`skills/agent-jsonl-compact-reader/SKILL.md` はバイナリに埋め込まれるため、docs に
見えても配布物が変わります。必ずリリース対象として扱います。

## 2. 版の決め方

| 変更 | 版 |
|---|---|
| 互換性契約の破壊（フラグ削除、JSON キー変更、schema / cursor bump、終了コード変更） | major（1.x 以降）/ minor（0.x） |
| 後方互換な追加（新 subcommand、新オプション、新フィールド、新 provider） | minor |
| 後方互換な修正・配布（バグ修正、docs、CI、installer、新 prebuilt arch、埋め込みスキル） | patch |
| 利用者に見える変化なし | リリース不要 |

迷ったら「既存の利用者スクリプトが壊れるか」で判断します。壊れるなら major（0.x では minor）です。

## 3. リリース前チェックリスト

リリース commit では次を同時に更新します。

- [ ] `Cargo.toml` の `version`
- [ ] `Cargo.lock`（`cargo update -p agent-jsonl-compact --offline` などで追従）
- [ ] `src/query/version.rs` の `SOURCE_RELEASE_DATE`
- [ ] 版・日付を固定しているテスト（例: `tests/viewer_e2e.rs::a11_version_consistency`）
- [ ] `CHANGELOG.md`（英語正本）と `CHANGELOG.ja.md`
- [ ] 変更に対応する README / ONBOARDING / RELEASING / ADR の日英ペア

検証:

```bash
just check        # fmt-check + clippy -D warnings + test + docs-check
just docs-check   # 日英ペアだけを単独で確認する場合
```

main へ push し、CI（Linux の check + macOS 2 target build + docs pair check）が green に
なることを確認します。tag を切る前に、必要なら release workflow を手動で dry-run できます
（`workflow_dispatch` では publish job は走らず、build artifact だけが残ります）。

```bash
gh workflow run release.yml --ref main
gh run list --workflow release.yml --limit 3
```

## 4. tag と Release

CI green 後、main の対象 commit に annotated tag を作成して push します。

```bash
git tag -a vX.Y.Z -m "agent-jsonl-compact X.Y.Z"
git push origin vX.Y.Z
```

`release.yml` が次の 3 target を matrix build し、1 つの Release へ添付します。

- `x86_64-unknown-linux-musl`
- `aarch64-apple-darwin`
- `x86_64-apple-darwin`

各 target は `agent-jsonl-compact-<target>.tar.gz` と `.sha256` を生成します。publish job は
全 build job の成功後にのみ実行されます。

## 5. 資産の検証

```bash
gh release view vX.Y.Z --json tagName,isDraft,isPrerelease,assets
```

期待する資産:

```text
agent-jsonl-compact-x86_64-unknown-linux-musl.tar.gz (+ .sha256)
agent-jsonl-compact-aarch64-apple-darwin.tar.gz      (+ .sha256)
agent-jsonl-compact-x86_64-apple-darwin.tar.gz       (+ .sha256)
```

`release-verify.yml` は Release published 時、または `release` workflow の成功後に、
Linux / macOS arm64 / macOS Intel の native runner で資産を download し、checksum と
release バイナリの smoke test を実行します。`GITHUB_TOKEN` で作成した Release は
`release: published` を発火しないため、workflow 完了を契機にする経路も用意しています。
手動で同じ検証を行う場合:

```bash
gh workflow run release-verify.yml -f tag=vX.Y.Z
```

ダウンロードして checksum と中身を確認します。

```bash
tmp="$(mktemp -d)"
gh release download vX.Y.Z --dir "$tmp"
( cd "$tmp" && sha256sum -c ./*.sha256 )
for a in "$tmp"/*.tar.gz; do tar -C "$tmp" -xzf "$a"; done
file "$tmp"/agent-jsonl-compact-*/agent-jsonl-compact
"$tmp/agent-jsonl-compact-x86_64-unknown-linux-musl/agent-jsonl-compact" --version
```

macOS バイナリは Linux 上では実行できません。`file` で Mach-O の arch を確認し、可能なら
macOS 実機で `--version` と `version --output json` を確認します。macOS バイナリは
ad-hoc 署名のみで notarization はしていません。Gatekeeper の隔離がある場合は
`xattr -d com.apple.quarantine <binary>` を案内します。

## 6. Release notes

自動作成された Release 本文を、必要なら CHANGELOG の該当節で置き換えます。

```bash
gh release edit vX.Y.Z --notes-file temp/release-notes-vX.Y.Z.md
```

少なくとも次を含めます。

- 版と固定 source release date
- 主な変更（`Breaking` があれば明記）
- 添付資産（3 target と checksum）
- macOS の署名 / notarization の注意

## 7. hotfix と不具合時

- 公開済み tag は移動・削除しません。
- 不具合時は最新 tag から branch を切り、patch を上げて修正し、main へ戻して新しい tag を切ります。
- 旧 Release に「使用しないこと」を追記する場合は `gh release edit` を使います。

## 8. pre-release

`vX.Y.Z-rc.N` の形式で tag を作ると、`release.yml` が prerelease として公開し、latest には
なりません。一般利用者には stable を案内します。

## 9. 制約

- 週次チェックは現時点では手動実行です。スケジュール実行は未設定です。
- macOS バイナリの build は CI で検証しますが、実行テストは Linux 上の Linux バイナリが中心です。
- Release の現在状態は本書に固定せず、GitHub Actions / Releases を確認してください。
