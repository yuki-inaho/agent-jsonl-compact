# 開発者・エージェント向けガイド

**日本語** | [English](ONBOARDING.en.md)

## 1. プロジェクトの概要

`agent-jsonl-compact` は、セッションJSONLの検索・閲覧・抽出を行うRust CLIです。
実行時にLLM・ネットワーク・外部エージェント基盤は不要です。
現在のバージョンは `Cargo.toml`、変更履歴は `CHANGELOG.md` を確認してください。
検証状況は、対象コミットのGitHub Actionsとローカルで実行した検査の結果で判断します。

抽出の既定は `*.clean.jsonl` のみです。`--format-out md|both` と `--summary` で追加ファイルを作ります。
`view` / `search` はstdoutへ結果を返すだけで、保存・索引作成はしません。
summaryは件数・パス等の情報であり、会話本文の代わりにはなりません。

## 2. 変更時に守る仕様

本文とツール出力の省略は明示オプションでのみ行います。
閲覧はreasoning・system・developer・compactionを常に除外しますが、抽出には同じ除外規則を適用しません。
実ログ・抽出物・認証情報をコミットしたり、外部へ送信したりしないでください。

閲覧の入力行サイズと表示量の上限、`partial` の条件、カーソルの再開・検証の仕様を維持してください。
`event_id` は `source_id:byte_start:subevent_index` で、cursor 継承の行番号を含めません。
`source.raw_line_verified=false` は行番号が未検証であることを示します。
抽出は行単位で読みますが、保存対象の全イベントをメモリに保持します。保存対象のデータ量が増えるほど、必要なメモリも増えます。
出力先ディレクトリからの脱出、入力ファイルの置換、シンボリックリンク先の上書きを拒否します。
Unixの保存ファイルは、所有者だけが読み書きできる権限にします。

最低対応Rustバージョンは `Cargo.toml` に、検証する依存パッケージのバージョンは `Cargo.lock` に定義します。
`SKILL.md` はビルド時に埋め込まれます。スキルの編集も再ビルド・配布の対象です。

## 3. 参照資料

| 資料 | 用途 |
|---|---|
| [README](../README.md) / [CLI](CLI.md) | 導入・操作・出力の仕様 |
| [入力形式](INPUT_FORMATS.md) | 各ログ形式への対応の根拠と制約 |
| [SECURITY](../SECURITY.md) | 検査範囲・機密情報の扱い |
| [ADR索引](adr/README.md) | 設計判断。歴史的な版情報はここに残す |
| [閲覧要件](requirements/session_viewer_cli.md) | F01–F10 / N01–N05と受入ケース |
| [リリース手順](RELEASING.md) | 版・タグ・配布物の検証 |
| `tests/fixtures/README.md` | 合成テストデータと匿名化サンプルの由来 |

## 4. 作業の範囲

パーサー・CLI・保存処理・インストーラー・テスト・文書の改善が対象です。
実ログは `temp/` などGitの管理対象外の場所へ置き、テストには合成データを使ってください。
公開済みタグの移動、実ログの公開、ライセンスの変更を通常の修正に含めないでください。

## 5. 作業の報告

報告は日本語で、何を変えたか、どのコマンドで確認したか、何が未検証かを簡潔に書きます。
過去の合格結果を現在の検証結果として転記しないでください。

## 6. 動作確認

```bash
just check
just demo
# demo-out/ に3入力 × 3種類 = 9ファイルができることを確認
find demo-out -type f
cargo run --locked -- view -i tests/fixtures/codex_sample.jsonl --tail 5 --output jsonl
cargo run --locked -- search -i tests/fixtures/codex_sample.jsonl --query hello --context 1
```

上は確認用手順です。自分で実行した結果にだけ「確認済み」を付けてください。
抽出ファイルは、拡張子を除いた入力ファイル名をもとに、clean・transcript・summaryを対応付けます。
本文が必要なときは、cleanかtranscriptが実際に保存されていることを確認してください。
閲覧結果は、メタデータの `status`、`counts`、`text_complete` を確認します。終了コード0だけで依頼された作業の完了とは判断しません。

## 7. 運用ルール・変更管理

利用手順はREADME、詳細な仕様はCLIリファレンス、変更履歴はCHANGELOGへ記載します。
利用・開発手順は日英ペアを同じ変更で更新し、設計判断が変わる場合はADRを追加します。
未確認事項には次に確認するファイルかコマンドを添えてください。

完了時は、依頼された成果物の実在、変更に対応するテストとlint、残る制約を確認します。
無関係なユーザーの変更を削除しないでください。macOSの実行結果やリモートActionsは、その環境での実行前に合格と報告しません。

### 実装の配置

| 実装 | 役割 |
|---|---|
| `src/cli.rs`, `src/lib.rs` | 引数・実行経路 |
| `src/detect.rs`, `src/classify.rs`, `src/clean.rs` | 抽出用の判定・正規化・フィルター |
| `src/runner.rs`, `src/render.rs` | ファイル保存・スキル配置・Markdown |
| `src/query/` | 上限付きの読み取り、表示範囲、検索、カーソル、ツール呼出しと結果の対応付け |
| `build.rs`, `src/query/version.rs` | `source_commit` と固定 source release date |
| `tests/security.rs`, `tests/test_installer.py` | 保存先の制約とインストーラーの回帰テスト |
| `tests/security_hardening_e2e.rs` | F-01〜F-09 の資源・表示・対応付け・cursor・探索の回帰 |
| `tests/test_justfile.py` | `just stats` の引数注入回帰 |
| `.github/workflows/`, `justfile` | 品質・セキュリティ・配布の検査。release は ci と security の両方を要求 |

CLIヘルプは `cargo run --locked -- --help`。依存関係は `Cargo.toml` / `Cargo.lock` を確認してください。
