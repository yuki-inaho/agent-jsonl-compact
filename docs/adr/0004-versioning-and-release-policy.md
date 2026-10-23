# ADR 0004 — バージョニングとリリース方針

**日本語** | [English](0004-versioning-and-release-policy.en.md)

- Status: **Accepted**
- Date: 2026-10-23
- 関連: [ADR 0003](0003-version-and-release-metadata.md), `Cargo.toml`, `src/query/version.rs`,
  `CHANGELOG.md`, `docs/RELEASING.md`, `.github/workflows/release.yml`

## 1. 問題

0.3.0 では閲覧 CLI の追加に伴って query schema v1 と cursor v1 を導入し、ADR 0003 で
「semver は `Cargo.toml`、更新日は固定 source date、公開済み tag は付け替えない」ことを
決めた。しかし、次の点が明文化されていなかった。

1. どの変更を major / minor / patch のどれとして扱うか
2. 0.x の間、互換性契約をどこまで守るか
3. 週次で「追加更新があるか」をどう判定し、いつリリースするか
4. 配布物（Linux / macOS）だけが増えた場合の版の扱い
5. hotfix と pre-release をどう扱うか

基準が無いと、docs のみの変更で不要なリリースをしたり、逆に埋め込み `SKILL.md` の
変更を配布し忘れたりする。判断を再現可能にする必要がある。

## 2. 決定

### 2.1 SemVer 2.0.0 を採用する

版は `MAJOR.MINOR.PATCH` とし、`Cargo.toml` の `version` を唯一の正本にする。
`src/query/version.rs` は `env!("CARGO_PKG_VERSION")` を読み、手書きの版文字列を置かない。

### 2.2 0.x の扱い

SemVer の仕様上 0.y.z は任意の変更を含み得るが、本プロジェクトは利用者の保護のため
次の独自ルールを置く。

- `0.y.z` の **patch** は後方互換。CLI 挙動、出力 schema、cursor、終了コードを変えない。
- `0.y+1.0` の **minor** は破壊的変更を含み得る。含む場合は CHANGELOG に
  `Breaking` と移行方法を明記する。
- 1.0.0 以降は通常の SemVer に従い、破壊的変更は major のみで行う。

### 2.3 変更分類

| 変更の種類 | 版 | 例 |
|---|---|---|
| 互換性契約の破壊 | major（1.x 以降）/ minor（0.x） | CLI フラグの削除・改名、JSON フィールドの削除・改名、query schema / cursor version の bump、終了コード契約の変更、`install-skills` の配置先変更 |
| 後方互換な追加 | minor | 新 subcommand、新オプション、新 JSON フィールド、新 provider 対応、コード変更を伴う新プラットフォーム対応 |
| 後方互換な修正・配布 | patch | バグ修正、docs、CI、installer の修正、新しい prebuilt アーキテクチャの追加（コード契約の変更なし）、埋め込み `SKILL.md` の更新 |
| リリース不要（次回まで保留可） | なし | 埋め込まれない docs の誤字、テスト・CI のみの追加、内部リファクタで利用者に見える変化が無いもの |

適用例:

- `0.3.0`: minor。`sessions` / `view` / `search` を追加し、既存抽出の schema・既定値・
  終了コードは維持した。
- `0.3.1`: patch。macOS prebuilt、release matrix、ADR 0004/0005、日英ドキュメントを追加。
  query schema / cursor / CLI 挙動は不変。

互換性契約として明示的に守るもの:

- legacy 抽出のフラグ、既定値、出力 schema、終了コード
- 閲覧系の query schema v1、cursor v1、JSON キー、終了コード `0` / `1` / `2`
- `install-skills` の配置先
- Release 資産名と `.sha256` の形式
- MSRV（引き上げは minor とし、CHANGELOG に明記する）

### 2.4 リリース要否と週次チェック

毎週、最新の `v*` タグと `origin/main` を比較する。再現コマンドは
`just weekly-check`（`scripts/weekly-release-check.sh`）。

- 差分が 0 件: リリースしない。
- `src/`、`Cargo.toml`、`Cargo.lock`、`skills/`、`install.sh` に変更:
  2.3 に従って分類し、リリースする。特に `skills/agent-jsonl-compact-reader/SKILL.md`
  はバイナリに埋め込まれるため、docs に見えても配布物が変わる。
- tests / docs / CI / metadata のみ: 原則として次回コードリリースへまとめる。テスト追加や
  ドキュメント修正だけではリリースしない。ただし配布やインストーラの修正を含む場合は
  patch を切る。
- 最終タグから 7 日以上経過し、リリース対象の変更がある場合は速やかにリリースする。

リリースには次を必須とする。

1. `just check` が green（`docs-check` を含む）
2. Linux x86_64 musl / macOS arm64 / macOS x86_64 の 3 資産が build できる
3. 各資産の `.sha256` が生成され、Release に添付される
4. `CHANGELOG.md` と `CHANGELOG.ja.md` に同じ版・日付がある

### 2.5 版の更新箇所

リリース commit で次を同時に更新する。

- `Cargo.toml` の `version`（`Cargo.lock` も追従）
- `src/query/version.rs` の `SOURCE_RELEASE_DATE`
- `CHANGELOG.md` / `CHANGELOG.ja.md`
- 版・日付を固定しているテスト（`tests/viewer_e2e.rs` の `a11_version_consistency` など）

### 2.6 tag、pre-release、hotfix

- リリースは main の merge 済み commit に対する annotated tag `vX.Y.Z` で行う。
- **公開済み tag は移動・削除しない。** 不具合時は新しい patch を切り、必要なら旧 Release に
  「使用しないこと」を追記する。
- pre-release は `vX.Y.Z-rc.N` とし、GitHub Release では prerelease として公開する。
  latest にはしない。一般利用者には stable を案内する。
- hotfix は最新 tag から branch を切り、patch を上げて main へ戻す。

### 2.7 `source_release_date`

`SOURCE_RELEASE_DATE` は「その source をリリースとして固定した日」であり、build 時刻でも
GitHub Release の公開時刻でもない。リリース commit で更新し、同じ source からは常に同じ値を
返す。ADR 0003 の決定を継承する。

## 3. 比較した代替案

| 代替 | 利点 | 採らなかった理由 |
|---|---|---|
| CalVer（日付ベース） | リリース日が分かりやすい | 互換性の大きさを表現できない。利用者が upgrade 判断をしにくい |
| コミット数や build 番号を版にする | 自動化しやすい | 利用者向けの互換性情報にならない。SemVer の契約と衝突する |
| docs 変更でも常にリリース | 配布が常に最新 | 空に近い Release が増え、利用者の upgrade 判断を邪魔する |
| 0.x でも破壊的変更をすべて patch に押し込む | 版番号が小さいまま | patch の後方互換契約を壊す。利用者が安全に upgrade できない |

## 4. 帰結

- 良: 版番号から互換性の大きさを判断できる。
- 良: 週次チェックとリリース要否がスクリプトで再現できる。
- 良: 埋め込みスキルの変更を配布し忘れない。
- 負: リリースのたびに複数箇所（Cargo / version.rs / CHANGELOG / テスト）を更新する必要がある。
- 負: 0.x の minor は破壊的変更を含み得るため、利用者は CHANGELOG の `Breaking` を確認する必要がある。

## 5. 検証

| 検査 | 方法 |
|---|---|
| 版の一致 | `tests/viewer_e2e.rs::a11_version_consistency` |
| 固定日付 | `src/query/version.rs` の unit test |
| 週次判定 | `just weekly-check`（read-only） |
| リリース資産 | `release.yml` の matrix build と Release 添付 |
| docs の日英同期 | `just docs-check` |

## 6. 限界

- 本 ADR は個々のリリースの実行結果を保証しない。現在状態は GitHub Actions / Releases を参照する。
- 変更分類は判断を伴う。迷う場合は「利用者の既存スクリプトが壊れるか」を基準にし、
  破壊的なら major（0.x では minor）として扱う。
- 週次チェックは現時点では手動実行であり、スケジュール実行は未設定である。
