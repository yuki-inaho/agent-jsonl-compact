# ADR 0005 — 二言語ドキュメント方針

**日本語** | [English](0005-bilingual-documentation.en.md)

- Status: **Accepted**
- Date: 2026-10-23
- 関連: `README.md` / `README.en.md`, `docs/ONBOARDING.md` / `.en.md`,
  `docs/RELEASING.md` / `.en.md`, `docs/README.md` / `.en.md`,
  `docs/adr/README.md` / `.en.md`, `scripts/check-docs.sh`, `.github/workflows/ci.yml`

## 1. 問題

本リポジトリの利用者・エージェントは日本語話者に限らないが、主要ドキュメントは日本語で
書かれてきた。英語話者が README だけを読んで誤った既定値や制約を理解するリスク、
日本語と英語で内容がずれるリスク、逆に全文を二重管理して更新コストが増えるリスクがある。

決める必要がある点:

1. どちらをデフォルトにするか
2. どの文書を二言語化するか
3. ファイル名と言語スイッチャーの形式
4. どちらを正本とし、どう同期するか
5. 翻訳で変えてはいけないもの

## 2. 決定

### 2.1 日本語をデフォルト、英語を `*.en.md` とする

- 日本語: `README.md`、`docs/ONBOARDING.md`、`docs/RELEASING.md`、`docs/README.md`、
  `docs/adr/README.md`、新しい ADR の `NNNN-*.md`
- 英語: 同じディレクトリの `README.en.md`、`ONBOARDING.en.md`、`RELEASING.en.md`、
  `docs/README.en.md`、`docs/adr/README.en.md`、`NNNN-*.en.md`

`README.md` の先頭に言語スイッチャーを置き、日本語を既定表示にする。英語版からも
日本語版へ戻れるようにする。

### 2.2 対象範囲

**Living docs（利用者・参加者が継続的に読む文書）は日英ペアを必須とする。**

- `README.md` / `README.en.md`
- `docs/ONBOARDING.md` / `docs/ONBOARDING.en.md`
- `docs/RELEASING.md` / `docs/RELEASING.en.md`
- `docs/README.md` / `docs/README.en.md`（ドキュメント索引）
- `docs/CLI.md` / `docs/CLI.en.md`（CLIリファレンス）
- `docs/INPUT_FORMATS.md` / `docs/INPUT_FORMATS.en.md`（入力形式）
- `SECURITY.md` / `SECURITY.en.md`（セキュリティ）
- `docs/adr/README.md` / `docs/adr/README.en.md`（ADR 索引）
- 新しく追加する ADR（`NNNN-*.md` / `NNNN-*.en.md`）

**Historical records（過去の設計・作業記録）は日本語を正本とし、英語では索引に要約を置く。**

- `docs/adr/0001`–`0003`（既存 ADR）
- `docs/requirements/session_viewer_cli.md`
- `docs/workdoc_session_jsonl_compactor_rust_port.md`

既存 ADR の全文英訳は必須にしない。`docs/adr/README.en.md` に各 ADR の英語サマリを置き、
必要な読者が日本語本文へ到達できるようにする。将来、全文英訳を追加する場合は
`NNNN-*.en.md` の命名に従う。

### 2.3 言語スイッチャー

各ペアの先頭に次の形式で置く。日本語側は日本語を強調し、英語側は英語を強調する。

```markdown
**日本語** | [English](README.en.md)
```

```markdown
[日本語](README.md) | **English**
```

サブディレクトリでは相対パス（例: `ONBOARDING.en.md`）を使う。

### 2.4 正本と同期

- 日本語版を利用者向けのニュアンスの正本とする。英語版はコマンド、既定値、制約、
  セキュリティ上の注意を省略してはならない。
- 日英は**同じ commit / PR で更新する**。片方だけの変更はレビューで差し戻す。
- 内容が衝突した場合、日本語版を正として英語版を修正する。ただしコマンドや JSON キーなど
  実行可能な契約は両方で同一でなければならない。
- 翻訳は逐語訳である必要はない。読みやすさのための短縮は許すが、警告・既定値・
  「やってはいけないこと」は落とさない。

### 2.5 翻訳しないもの

コード識別子、CLI フラグ、subcommand 名、JSON キー、終了コード、ファイルパス、URL、
環境変数、schema version、`source_release_date` などの契約値は原表記のままにする。

### 2.6 CHANGELOG の例外

`CHANGELOG.md` は英語を正本とし、`CHANGELOG.ja.md` を日本語版とする。既存のテスト
（`CHANGELOG.md` を読む `a11_version_consistency`）と一般的な changelog ツールの慣習に
合わせるためである。両方に同じ版・日付を記載する。

### 2.7 検査

`scripts/check-docs.sh` が次を検査する。

- 対象ファイルの日英ペアが存在する
- 各ファイルが相手のファイル名へのリンクを含む

`just docs-check` と CI（`.github/workflows/ci.yml`）で実行する。リリース手順
（`docs/RELEASING.md`）にも `just docs-check` を必須項目として含める。

## 3. 比較した代替案

| 代替 | 利点 | 採らなかった理由 |
|---|---|---|
| 英語をデフォルトにする | 国際的な利用者に即座に届く | 日本語話者の maintainer / 利用者が日常的に読む文書として自然でない。既存資産も日本語 |
| 1 つの文書に日英を併記する | ファイルが増えない | 文書が長くなり、言語スイッチャーも成立しない。更新時の差分も読みにくい |
| 機械翻訳だけを置く | 低コスト | 既定値・警告・契約のずれを検出できず、正本が曖昧になる |
| 英語版を別リポジトリに分ける | 読者層を分けられる | 同期コストとリンク切れのリスクが増える |

## 4. 帰結

- 良: 日本語話者は従来どおり日本語を既定で読め、英語話者も同じ内容へ到達できる。
- 良: 言語スイッチャーと検査により、片方だけの更新を検出できる。
- 良: 歴史的記録の全文翻訳を必須にしないため、維持コストを抑えられる。
- 負: living docs の更新は常に 2 ファイルの更新を伴う。
- 負: 英語版の微妙なニュアンスは日本語版より劣る可能性がある。衝突時は日本語を正とする。
- 負: 既存 ADR 0001–0003、要件、作業記録は英語全文ではない。

## 5. 検証

| 検査 | 方法 |
|---|---|
| ペアの存在 | `scripts/check-docs.sh` |
| 相互リンク | `scripts/check-docs.sh` |
| CI 強制 | `.github/workflows/ci.yml` の `Docs language pairs` |
| リリース時の確認 | `docs/RELEASING.md` のチェックリスト |

## 6. 限界

- 検査はファイルの存在とリンクのみで、翻訳内容の意味的な一致は検証しない。
- 英語版が古いまま残る可能性はゼロではない。レビューとリリースチェックリストで補う。
- 既存 ADR の英語サマリは全文ではなく、判断の要点のみである。
