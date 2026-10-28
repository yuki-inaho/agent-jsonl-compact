# ADR 0006 — 依存関係の更新と安全な保存・導入

**日本語** | [English](0006-safe-installation-and-storage.en.md)

- Status: Implemented
- Date: 2026-10-23
- 関連: [ADR 0003 §2.4](0003-version-and-release-metadata.md)、[ADR 0004](0004-versioning-and-release-policy.md)

## 問題と根拠

セキュリティ・導入手順の見直し依頼を受け、現行依存グラフと保存処理を確認した。
`cargo-audit 0.22.2` の `cargo audit --deny warnings` は、`time 0.3.36` に
[RUSTSEC-2026-0009](https://rustsec.org/advisories/RUSTSEC-2026-0009)、`anyhow 1.0.102` に
[RUSTSEC-2026-0190](https://rustsec.org/advisories/RUSTSEC-2026-0190) を報告して終了コード1になった。

前者の対象は RFC 2822 の日時解析、後者の対象は `Error::downcast_mut` で、現在のコードはその API を使っていない。
ただし、ADR 0003でRust 1.74を維持するために固定した `time` を更新できず、依存グラフの監査が失敗する状態だった。
修正版 `time 0.3.47` はRust 1.88以上を要求する（`cargo info time@0.3.47`）。

保存処理は出力名をパスへ連結し、`File::create` / `fs::write` で直接書き込んでいた。
名前による出力先の脱出、リンク先・入力自体の上書き、Unixでの公開権限、スキルの編集内容の無条件置換が起こり得た。
インストーラーはアーカイブ全体を展開し、見つかった最初の同名バイナリを配置していた。

## 決定

MSRVを1.88へ引き上げ、`time` の下限を0.3.47、`anyhow` の下限を修正版1.0.104にする。
ADR 0003 §2.4のMSRV維持の決定をこの範囲で置き換える。版・source date・schemaの契約は変更しない。
`Cargo.lock` を更新し、CIでMSRVの `cargo test --locked` を実行する。

抽出名はパス区切り・制御文字を含まない単一の名前に制限する。
全出力先を事前検査し、入力と同じ実体や通常ファイル以外の出力先を拒否する。
`tempfile` を本番依存として使い、同じディレクトリの一時ファイルから出力先を置き換える。
Unixでは出力を0600にする。既存のhardlinkの別名は書き換えない。
統計・パス・エラーの端末表示では制御文字をエスケープし、保存する本文は変更しない。

スキルの既存ファイルは保持し、置換には `--force` を要求する。
スキル本体のディレクトリや `SKILL.md` のsymlinkは、forceでも上書きしない。

インストーラーはタグ・checksum・メンバー名・バイナリの通常ファイル種別を検査する。
検査したバイナリだけをstdout経由で取り出し、配置先の一時ファイルから置き換える。
ダウンロードしたバイナリの自動実行と、隔離属性を無条件に外す案内をやめる。
SHA-256は署名検証ではないことを導入手順に明記する。

security workflowで既知脆弱性と警告、ShellCheck、保存・インストーラーの回帰を検査する。
read権限で実行し、release buildの前提条件にする。Actionsはcommit SHAへ固定する。
READMEにはセキュリティバッジを追加しないという利用者の指定に従う。

## 比較した案

| 案 | 判断 |
|---|---|
| Rust 1.74と旧依存を維持し、対象APIを使わない理由で監査を除外 | 今回の呼出し経路に到達性はないが、除外を維持する運用が必要になるため採らない |
| 日時ライブラリを交換・独自実装してMSRVを維持 | 日時境界の互換性検証を広げる必要があり、今回の修正として採らない |
| 修正版へ更新しMSRVを引き上げる | 採用。ソース利用者の要件は変わるが、prebuilt利用者にRustを要求しない |
| 既存出力をすべて拒否する | 通常の再抽出を変えるため採らない。安全に名前を置き換える |

## 検証と限界

`tests/security.rs` は出力名・入力同一性・symlink・hardlink・0600・既存スキルとforceを検証する。
`tests/test_installer.py` は実際のtarとchecksum処理を使い、ネットワークだけを合成リリースへ差し替える。
Linux上でMSRVの全Rustテスト、インストーラーテスト、更新後の `cargo audit --deny warnings` が成功した。

macOSのローカル実機検証と、この変更のリモートActions実行は未実施。
アーカイブの取得元認証、未知の脆弱性、悪意ある共有親ディレクトリ、複数出力の一括rollbackは保証しない。
次の公開ではMSRV引上げ・スキルの既存ファイル保持・出力先制限をBreakingとして案内する。
