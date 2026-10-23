# LLMオンボーディングサマリー

**日本語** | [English](ONBOARDING.en.md)

## 1. プロジェクト概要と目的

`agent-jsonl-compact` は、セッションJSONLの検索・閲覧・抽出を行うRust CLIです。
実行時にLLM・ネットワーク・外部エージェント基盤は不要です。
現在の版は `Cargo.toml`、変更履歴は `CHANGELOG.md`、検証状況はActionsと実行結果を確認してください。

抽出の既定は `*.clean.jsonl` のみです。`--format-out md|both` と `--summary` で追加ファイルを作ります。
`view` / `search` はstdoutへ結果を返すだけで、保存・索引作成はしません。
summaryは件数・パス等の情報であり、会話本文の代わりにはなりません。

## 2. クリティカルな要求・制約

本文とツール出力の省略は明示オプションでのみ行います。
閲覧はreasoning・system・developer・compactionを常に除外しますが、抽出には同じ除外規則を適用しません。
実ログ・抽出物・認証情報をコミットしたり、外部へ送信したりしないでください。

閲覧のrecord上限・表示上限・partial・cursorの契約を保ちます。
抽出は行単位で読みますが、採用後の全eventをメモリに保持します。無制限の入力を安全に処理できるとは扱いません。
出力名による保存先の脱出、入力の置換、symlink先の上書きを拒否し、Unixの保存ファイルは所有者だけが読み書きできる権限にします。

Rustの最低要件は `Cargo.toml`、検証する依存グラフは `Cargo.lock` が正本です。
`SKILL.md` はビルド時に埋め込まれます。スキルの編集も再ビルド・配布の対象です。

## 3. 参照すべき合意済み資料

| 資料 | 用途 |
|---|---|
| [README](../README.md) / [CLI](CLI.md) | 導入・操作・出力契約 |
| [入力形式](INPUT_FORMATS.md) | providerごとの根拠と制約 |
| [SECURITY](../SECURITY.md) | 検査範囲・機密情報の扱い |
| [ADR索引](adr/README.md) | 設計判断。歴史的な版情報はここに残す |
| [閲覧要件](requirements/session_viewer_cli.md) | F01–F10 / N01–N05と受入ケース |
| [リリース手順](RELEASING.md) | 版・タグ・配布物の検証 |
| `tests/fixtures/README.md` | 合成fixtureと匿名化サンプルの由来 |

## 4. タスク境界（任せること / 任せないこと）

パーサー・CLI・保存処理・インストーラー・テスト・文書の改善が対象です。
実ログは `temp/` などのignore対象へ置き、fixtureは合成データを使ってください。
公開済みタグの移動、実ログの公開、ライセンスの変更を通常の修正に含めないでください。

## 5. インタラクション方針

報告は日本語で、何を変えたか、どのコマンドで確認したか、何が未検証かを簡潔に書きます。
過去の合格結果を現在の検証結果として転記しないでください。

## 6. 試行タスク（オンボーディング演習）

```bash
just check
just demo
# demo-out/ に3入力 × 3種類 = 9ファイルができることを確認
find demo-out -type f
cargo run --locked -- view -i tests/fixtures/codex_sample.jsonl --tail 5 --output jsonl
cargo run --locked -- search -i tests/fixtures/codex_sample.jsonl --query hello --context 1
```

上は確認用手順です。自分で実行した結果にだけ「確認済み」を付けてください。
抽出は同じ入力stemのclean・transcript・summaryを対応付け、本文が必要ならcleanかtranscriptの実在を確認します。
閲覧はmetadataの `status`・`counts`・`text_complete` を確認します。終了コード0だけで依頼作業の完了とは判断しません。

## 7. 運用ルール・変更管理

利用手順はREADME、詳細契約はCLI、変更履歴はCHANGELOGへ記載します。
継続して読む文書は日英ペアを同じ変更で更新し、設計判断が変わる場合はADRを追加します。
未確認事項には次に確認するファイルかコマンドを添えてください。

完了時は、依頼された成果物の実在、変更に対応するテストとlint、残る制約を確認します。
無関係なユーザーの変更を削除しないでください。macOSの実行結果やリモートActionsは、その環境での実行前に合格と報告しません。

### 付録: 参考情報

| 実装 | 役割 |
|---|---|
| `src/cli.rs`, `src/lib.rs` | 引数・実行経路 |
| `src/detect.rs`, `src/classify.rs`, `src/clean.rs` | 抽出用の判定・正規化・フィルター |
| `src/runner.rs`, `src/render.rs` | ファイル保存・スキル配置・Markdown |
| `src/query/` | bounded reader・scope・検索・cursor・tool pair |
| `tests/security.rs`, `tests/test_installer.py` | 保存境界とインストーラーの回帰テスト |
| `.github/workflows/`, `justfile` | 品質・セキュリティ・配布の検査 |

CLIヘルプは `cargo run --locked -- --help`。依存関係は `Cargo.toml` / `Cargo.lock` を確認してください。
