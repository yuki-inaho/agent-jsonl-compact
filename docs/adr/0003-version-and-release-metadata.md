# ADR 0003 — 版とリリースメタデータ

- Status: **Accepted / Implemented / 本記録は局所検証（local evidence）まで。**
  tag・release の現在状態は [Releases](https://github.com/yuki-inaho/agent-jsonl-compact/releases) /
  [GitHub Actions](https://github.com/yuki-inaho/agent-jsonl-compact/actions) を参照。
- Date: 2026-10-21
- 関連: [ADR 0001](0001-additive-session-viewer-cli.md), `src/query/version.rs`, `Cargo.toml`, `CHANGELOG.md`

## 1. 問題

利用者が「どの版か」「いつ固定された版か」を CLI から確認できる必要がある（要件 G4）。
一方で、build 時刻や GitHub Release の更新時刻を「更新日」として代用すると、
同じ source から作ったバイナリで値が変わり、再現性が失われる。
また 0.3.0 は additive minor であり、既存 0.2.0 利用者の schema と終了コードを壊せない。

決定すべき点:

1. semver の単一情報源
2. 「更新日」の定義（build 時刻か、固定 source date か）
3. `--version` / `version --output json` / `CHANGELOG` の一致
4. MSRV（1.74）を維持したまま依存を足せるか
5. tag が何を指すか、release 成果物をどう検証するか

## 2. 決定

### 2.1 版: 0.3.0（additive minor）、query schema v1

- `Cargo.toml` の `version = "0.3.0"` が semver の唯一の正本。
- `src/query/version.rs` は `env!("CARGO_PKG_VERSION")` を読む。手書きの版文字列を置かない。
- 新規閲覧出力は `schema_version: 1`（`src/query/model.rs` の `QUERY_SCHEMA_VERSION`）。
  legacy clean schema は変更しない。
- cursor 形式は `CURSOR_VERSION = 1` として別に版を持つ。

### 2.2 更新日: 固定 source release date

- `SOURCE_RELEASE_DATE = "2026-10-21"`（`src/query/version.rs` の定数）。
- `version --output json` は `name` / `version` / `source_release_date` /
  `query_schema_version` / `cursor_version` / `source_date_kind` を返す。
  `source_date_kind` は `fixed_release_date_not_build_time` であり、
  build 時刻ではないことを機械可読に示す。
- `--version` は従来どおり semver のみ（`clap` の `version`）。既存利用者の出力を変えない。
- **GitHub Release の `updated_at` は「最後に編集された時刻」、`published_at` は「公開時刻」**
  であり、どちらも固定 source date と比較しない。3つは別の意味を持つ。検証対象は「Cargo 版 = JSON 版 = CHANGELOG の版」と
  「CHANGELOG と JSON の source date」である。
- revision（build した commit SHA）は、不明な場合に捏造するより省略する。

### 2.3 tag と release

- merge 済み main の SHA に対して **annotated tag `v0.3.0`** を作成し push する。
- CI（`.github/workflows/ci.yml`）は PR 必須ゲート: `cargo fmt --all --check`、
  `cargo clippy --all-targets -- -D warnings`、`cargo test`。
- release（`.github/workflows/release.yml`）は tag push で起動し、musl 静的バイナリの
  `tar.gz` と `.sha256` を GitHub Release へ添付する。`install.sh` はこの資産を sha256 検証する。
- 公開済み tag の付け替えはしない。build 失敗時は原因を記録し、修正版の扱いを別途判断する。

### 2.4 MSRV 1.74 の維持

0.3.0 で追加した依存と、その固定理由:

| crate | 指定 | 理由 |
|---|---|---|
| `regex` | `1` | 検索の `--regex` |
| `sha2` | `0.10` | anchor / source identity のハッシュ |
| `base64` | `0.22` | cursor の不透明エンコード |
| `time` | `=0.3.36` | RFC3339 parse。1.74 で通ることを確認済みの版に固定 |
| `clap` | `>=4.5.20, <4.6` | 4.6 系は edition 2024（rustc 1.85 要求）のため除外 |
| `tempfile`（dev） | `=3.10.1` | 新しめの版は getrandom 0.4（edition 2024）を引く |

- **「最新の互換版」という主張はしない。** 各ピンは「1.74 で通ることを確認した版」であり、
  それ以上でも以下でもない。Cargo.toml のコメントにも同じ限定を記している。
- 検証は `cargo +1.74.0 build` / `cargo +1.74.0 test <changed paths>` で行う。
  全経路ではなく、変更した crate と feature の組合せを対象にする。

## 3. 比較した代替案

| 代替 | 利点 | 採らなかった理由 |
|---|---|---|
| build 時刻を更新日に使う | 実装が単純 | 同じ source でも値が変わり再現しない。利用者の「版の日付」の意味とずれる |
| Git commit 時刻 | 自動で決まる | rebase/cherry-pick で変わる。公開後の再現性が無い |
| GitHub Release の `updated_at` / `published_at` を正とする | 公開事実と一致 | 編集・再公開のたびに変わる。source の版日付ではない |
| Cargo 版だけを出す | 最小 | 利用者が「いつ固定されたか」を確認できない（G4） |

## 4. 帰結

- 良: 同じ source からは常に同じ `source_release_date` が出る。
- 良: 版の一致を3箇所（Cargo / JSON / CHANGELOG）で機械的に検査できる。
- 負: リリースのたびに定数と CHANGELOG を手で更新する必要がある（単一情報源は semver のみ）。
- 負: MSRV ピンにより、依存の更新は「1.74 で通るか」の確認を伴う。
- 本書は CI 通過・merge・tag 作成・release 公開を**主張しない**。これらは時点で変わるため、
  固定の「未実施」ではなく現在状態を GitHub Actions / Releases で確認する。

## 5. 証拠

| 検査 | 方法 |
|---|---|
| 版の一致 | `tests/viewer_e2e.rs::a11_version_consistency` が `--version` と `version --output json` を比較 |
| 単一定数 | [src/query/version.rs](../../src/query/version.rs) の unit test `version_matches_cargo_and_date_is_fixed` |
| legacy 互換 | `a11_legacy_outputs_and_exit_codes_stable` |
| installer | `a11_installer_matrix_nine_cases`（空/既存同内容/既存別内容 × all/claude-only/codex-only） |
| MSRV | `cargo +1.74.0 build` を変更 crate に対して実行し、receipt を残す |

再現:

```bash
rtk proxy cargo test --test viewer_e2e a11_version_consistency
rtk proxy cargo test --test viewer_e2e a11_installer_matrix_nine_cases
rtk proxy cargo +1.74.0 build
./target/debug/agent-jsonl-compact version --output json
```

## 6. 限界

- 公開後の release 資産（tarball / checksum）の検証は、tag 作成後にしか行えない。
- `source_release_date` は「この source が 0.3.0 として固定された日」であり、
  個々の機能の実装日でも、build 日でも、公開日でもない。
- CI の実行結果は本 ADR には含めない。必須ゲートとして定義するのみである。

## 7. 追補 — 0.3.1（2026-10-23）

- 0.3.1 は配布のみの patch。query schema v1、cursor v1、legacy schema、終了コードは
  0.3.0 から不変。
- `SOURCE_RELEASE_DATE` は 0.3.1 の固定 source date `2026-10-23` に更新した。
- `release.yml` は Linux x86_64 musl に加え、macOS `aarch64-apple-darwin` /
  `x86_64-apple-darwin` を matrix build し、同じ Release に添付する。
- 公開済み `v0.3.0` の tag と assets は変更しない。
- バージョンの分類基準は [ADR 0004](0004-versioning-and-release-policy.md)、日英ドキュメントの
  運用は [ADR 0005](0005-bilingual-documentation.md) に分離した。

## 8. 追補 — 0.4.0（2026-10-23）

- 0.4.0 は 0.x minor。抽出の既定出力を `<name>.clean.jsonl` のみに変更した（Breaking）。
  transcript は `--format-out md|both`、summary は `--summary` で明示する。
- query schema v1、cursor v1、legacy clean schema、終了コードは不変。
- `SOURCE_RELEASE_DATE` は 0.4.0 も `2026-10-23`（同日リリースのため）。
