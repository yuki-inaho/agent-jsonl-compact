# agent-jsonl-compact：会話を探して必要な箇所を読むCLIの要件

> この文書は作成当時の要件・検証記録です。版・日付・試行結果は履歴として保持します。現在の利用方法は [README](../../README.md) と [CLIリファレンス](../CLI.md) を確認してください。

作成日：2026-10-21。対象：CLI 0.2.0（初稿）→ 0.3.0 で実装。
source `290c6d7`（初稿時点）。

**状態：P0（F01–F10 / N01–N05）は 0.3.0 として実装済み・本記録時点で局所検証済み。**
CI・merge・tag・release は時点で変わるため本書では固定せず、現在状態は
[GitHub Actions](https://github.com/yuki-inaho/agent-jsonl-compact/actions) /
[Releases](https://github.com/yuki-inaho/agent-jsonl-compact/releases) を参照する。
本書は将来の通過を主張しない。
初稿時に確認した 0.2.0 の観察（§2 の表）は当時の記録としてそのまま残す。
実装と検証の対応は §9 にまとめた。

## 1. 何を解決するか

ユーザー／調査エージェントが、対象の会話を見つけ、「今日の依頼」「直近の回答」「ある処理を実行した根拠」を、全文を書き出さずに読めるようにする。

現行の忠実な縮約・ファイル出力は残す。その前後で毎回書いているsession探索、role／日時filter、keyword検索、前後文脈、raw tool読戻し、追記差分の処理をCLIへまとめる。意味の要約や作業完了の判定は利用者が行い、CLIは原文と位置・取得範囲を返す。

想定利用者は、shellで調査する人と、限られた出力量で資料を読むエージェント。初期版は非対話CLIとし、TUIやWeb viewerの完成を前提にしない。

### 今回の範囲

- **納品範囲**：実利用に基づく要件、優先順位、操作案、受入ケース、改善の検証方法。
- **次の実装範囲案（MVP）**：session発見、bounded view/search、原文位置、tool対応、cursor差分、既存抽出の互換性。
- **初期版の対象外**：自動要約LLM、ベクトルDB、常駐daemon、プロセス操作、会話の編集／削除、暗号化reasoningの復元、agentへの指示送信。
- OpenCodeの既存run JSONLは既存対応を維持する。`opencode.db`やexport単一JSONからの新規取込みは別要件とする。

## 2. 調査で確認した使われ方

関連する長期Codex会話と最新Claude候補を選んだ目的抽出であり、全エージェントの利用頻度を推計した調査ではない。Codexは9月開始でも10月21日に追記された会話を含む。対象Claudeの最終記録は10月20日以前であり、調査時に実行中とは断定していない。

| ケース | 観察した操作 | 利用目的／現在の手間 |
|---|---|---|
| U01 | 日付folder・mtime・cwdを自作scriptで調べ、別日に開始した長期sessionを探し直す | 会話の開始日と今日の活動日が異なる。repo名だけでも担当が一意に決まらない |
| U02 | clean JSONLをPythonでrole・日時に絞り、最新26発言を表示する | 抽出後も「直近の依頼／回答を読む」ための処理が必要 |
| U03 | 更新された会話54.5MBを再抽出して3,551eventsを書き出し、末尾18eventsを読む | 更新確認のために全量再変換する手間がある。差分実装済みという意味ではない |
| U04 | 公開会話だけの中間入力を自作し、api／no-dedupで再抽出、rawとの件数・本文を照合する | 忠実な保存と閲覧用filterを分離したい |
| U05 | keyword検索→custom event索引の時刻・call_id→周辺会話と辿る | 生成物の来歴を調べる。`ts`を`timestamp`として読み、時刻がnullになる実例もある |
| U06 | 過去の表示指定を会話から再確認し、artifactの旧版／新版を区別して可視化を再出力する | 発言の再掲と、新しい依頼・対象版を区別して追う必要がある |
| U07 | Claude担当がCodexログを日時でsliceし、Markdownのencrypted reasoning見出しを除き見出し境界で分割する | 軽量化済みでも、読める大きさ・内容への再加工が必要 |
| U08 | OpenCodeの単一export JSONを自作変換し、対応run JSONLとして読む | role／model／cwdが正しく保持されるとは限らず、変換のlossを明示する必要がある |
| L01 | 調査側が既読byte位置から追記だけを抜き、provider metadataを付けて再抽出する | 1.3GB超の会話を毎回再読・再出力したくない |
| L02 | session一覧のtitleに長いコード／委託promptが入る | 一覧・metadataだけでも出力量が膨らむ。titleもboundedにする必要がある |

L01/L02は今回の調査側自身の操作・観察であり、独立した別agentの事例数へ加算しない。L02のtitleは41,070文字だった。原文のsession／時刻／raw行／byte範囲と抽出receiptはローカル証跡に保存し、実会話全文・個人pathは本要件書へ転載しない。

### 現行機能と、probeで確かめた限界

| 現状 | 根拠と解釈 |
|---|---|
| 3形式の判別・正規化、JSONL／Markdown／summary出力、cut／dedup、stats、skill installがある | [CLI定義](https://github.com/yuki-inaho/agent-jsonl-compact/blob/main/src/cli.rs)、[runner](https://github.com/yuki-inaho/agent-jsonl-compact/blob/main/src/runner.rs)。新規に同じ抽出器を別実装しない |
| 新Codex fixtureでterminalは本文を出さず、apiでuser／assistant本文が出る | 11 raw recordsのprobe：terminal 6 events（message本文0）、api 6（本文各1）、both 8。件数だけで閲覧成功と判定できない |
| custom call/outputはどのchannelでも落ちる | 同fixtureのraw各1件、計2件が欠落。通常function call/output各1件は保持。全providerで全toolが落ちるという意味ではない |
| 日時／role／検索／context／page／tail／follow／sessionsは現行CLIにない | 個別probeは未認識引数／subcommandとしてrc2。書き出したcleanへ外部commandを組み合わせれば代用できる |
| rawは行単位で読むが、採用した全EventをVecへ蓄積する | [reader](https://github.com/yuki-inaho/agent-jsonl-compact/blob/main/src/util.rs)、[runner](https://github.com/yuki-inaho/agent-jsonl-compact/blob/main/src/runner.rs)。`--stats`もこの経路。raw全文一括loadとは言わない。RSSは未計測 |
| raw行番号は内部で付くが出力eventへ残らない | 調査用索引を別作成する必要がある。clean行番号はraw行番号と異なる |
| 不完全JSONのparse_errorを残してrc0で終わる例がある | parse_errorの有無と完全な読取りを、終了codeだけから判断できない |
| skill installerは埋込SKILL.mdを配布する | supporting reference/scriptを追加する場合はinstaller側の対応も要る。文書だけ追加して配布完了とはしない |
| auto判別が本文を見ずにCodexへfallbackする例がある | 50件のunknown metadataの後にClaude本文を置く人工fixtureではautoで0event、明示formatで2event。一般的な誤判定率を示すものではない |
| 同じ本文の再送がtoolを挟んでも既定dedupで消える例がある | 閲覧系では本文一致だけで別発言を落とさない。既存exportの互換性とは分ける |

## 3. 要件と優先順位

P0はMVPの必須、P1は後続。CLIの綴り・既定上限は第4節の設計案として分ける。

| ID／優先 | 要件 | 根拠／受入 |
|---|---|---|
| F01 P0 | provider、session ID、cwd、最終event時刻で会話候補を探せる。開始日folderに限定しない | U01／A01 |
| F02 P0 | user／assistantの発言を既定で読み、時刻・role・phaseで絞れる。framework注入と判明したものを区別する | U02,U07／A02,A03 |
| F03 P0 | keywordまたは明示regexで検索し、ヒットの前後eventと元eventへ戻れる | U05,U06／A04 |
| F04 P0 | 対応toolの呼出しと結果をcall_idで辿れる。custom形式とClaude tool_use/resultを含む | U05／A05 |
| F05 P0 | eventからraw入力の位置へ戻れる。表示の切断、除外・未対応、取得境界を機械可読にする | U04,U05／A06 |
| F06 P0 | 既読cursor以後の追記だけ読める。異なる入力・queryへのcursor流用、切詰め等を明示拒否する | U03,U07,L01／A07,A08 |
| F07 P0 | 表示event数と出力byte数を制限できる。巨大title・本文・tool出力が上限を迂回しない | U07,L02／A09 |
| F08 P0 | 新しい閲覧系commandは保存先を要求せずstdoutへ返す。JSONLとstderr診断を分離する | U02,U05／A10 |
| F09 P0 | legacy `-i/-o`抽出・既存schema・install選択flagを壊さない | 既存機能／A11 |
| F10 P0 | sourceを変更せず、形式未確定・未対応・不正recordと未完の末尾を区別して報告する | U08,固定境界／A12 |
| F11 P1 | `follow`で追記を監視し、途中で止めても既読位置を再利用できる | F06が先。常駐daemon不要 |
| F12 P1 | 親子session、fork、別worktreeの候補を補助表示する | 確認できたmetadataのみ。cwd／titleから関係を断定しない |
| F13 P1 | stdin入力、選択範囲のexport、再利用cacheを整備する | source位置・再開の意味をfile入力と分ける。独自DBの常時構築は必須にしない |

### 非機能の必須条件

- **N01**：queryのために採用event全件を保持しない。メモリは入力総量で増えず、window・上限付きrecord処理・必要なmetadataに依存する構成とする。最大record処理byteを設定でき、入力段階で上限を適用する。表示だけをcutして低メモリ対応と呼ばない。
- **N02**：初回検索は走査を許す。cursor読取りは確認用anchorと追記部分を主に読み、毎回の全prefix hash／全量exportを必須にしない。
- **N03**：cold／warm、first page／全検索／差分、読取byte、出力byte、wall time、peak RSSを分けて計測する。現時点で「何倍高速」とは約束しない。
- **N04**：query系は既定で新しいファイル・indexを作らない。任意cacheは場所・無効化条件・source変更を明示する。元ログ・provider DBはread-only。
- **N05**：端末の制御文字を安全に表現し、ログ中のcommandを実行しない。実会話のuploadや外部model呼出しをCLIの必須動作にしない。

## 4. 操作の提案

**この節は将来のインターフェース案であり、0.2.0では実行できない。**

増やす入口は` sessions / view / search `の3つを基本とし、既存抽出を残す。`view`と`search`は同じ正規化・filter・出力・source位置の実装を共有する。

```sh
# 対象repoの会話候補を探す。IDはproviderで名前空間を分ける
agent-jsonl-compact sessions --provider codex --cwd /path/to/repo --limit 20

# 直近の公開発言。--sessionと-iは排他
agent-jsonl-compact view --session codex:SESSION_ID --tail 30
agent-jsonl-compact view -i /path/to/rollout.jsonl \
  --since 2026-10-21T00:00:00+09:00 --role user,assistant --output jsonl

# 本文の検索と前後event。正規表現は--regexで明示
agent-jsonl-compact search -i /path/to/rollout.jsonl \
  --query contour_v2 --context 2 --limit 10

# ヒットやevent_idから、対応toolの結果も読む
agent-jsonl-compact view -i /path/to/rollout.jsonl \
  --event EVENT_ID --scope tools --with-pair --output jsonl

# 前の返却metadataに含まれたcursorから追記を読む
agent-jsonl-compact view -i /path/to/rollout.jsonl \
  --after-cursor APPEND_CURSOR --output jsonl

# 同じ固定snapshotの続きをページ送りする
agent-jsonl-compact view -i /path/to/rollout.jsonl \
  --cursor PAGE_CURSOR --output jsonl
```

現行で使える保存commandは引き続き以下。新機能が入るまでは、現行skillのsummary→必要箇所という読み方を使う。

```sh
agent-jsonl-compact -i /path/to/rollout.jsonl -o /path/to/extract --channel api
```

### 選択・順序・表示の契約

1. 入力は`-i`またはprovider付き`--session`で一意に決める。候補重複・不明IDで勝手に別sessionを選ばない。ファイル開始日と最終event時刻を両方表示する。
2. `sessions`の探索対象はproviderごとの既知session rootと、明示された追加root。symlinkを辿るか、子sessionを含めるか、走査上限と未読範囲を表示する。provider DBを使う場合はread-only adapterの版とmetadata更新時刻を示す。`--cwd`はmetadata上のdirの完全一致を既定とし、子dirを含める指定は別flagとする。cwdは会話内で実際に編集している全repoを保証しない。最終更新だけで実行中・停止中を判定しない。
3. 日時はoffset付きISO8601、`since <= event_time < until`。元時刻も保持する。時刻欠損はunknownとして数え、mtimeで補わない。期間queryでは既定で結果から除外し、除外数を返す。
4. 通常は入力内の原文順。timestampの同値・逆行で勝手に並べ替えない。複数session検索はP1とし、順序規則を別途定義する。
5. `tail N`はfilter後の末尾N件を原文順で返す。event／cursor指定との併用は禁止。contextには**hard scope**と**soft filter**を分ける。hard scopeはmessages／tools等の明示した範囲と内部内容の除外で、contextでも越えない。`context K`はこのhard scope内の前後K eventを同じsessionから返す。role・時刻等のsoft filterを越えた隣接eventはcontextとして明示し、重複contextは一度だけ表示する。
6. 既定scopeはmessages。toolを見る場合は`--scope tools`、metadataを含める場合は明示指定する。内部reasoning・system/developer・compaction本文を公開会話へ混ぜない。user型でもskill注入・framework生成と判明するものは人の依頼とは区別し、既定messagesから除外して件数を示す。出自不明はunknownのまま表示し、人が入力したと断定しない。復元不能な内容は復元しない。
7. text表示は読める見出しと原文位置、JSONLはschema付きeventと最後のmetadata record。診断・進捗はstderrへ。新commandは**rc0＝対象範囲を正常処理（正当な0件を含む）、rc1＝source／decode／形式未確定／partial、rc2＝引数・query構文不正**とする。legacyの終了codeは変えない。0件は正常な全対象走査の場合のみ確定し、未検査範囲を残す場合はpartialとして扱う。
8. 件数・byte上限に達したら、切断の有無と継続方法を返す。暫定案はview 50件、search 20hits、総出力32KiB。stdout＋stderrの合計を上限とし、metadata・title・診断も含める。診断は集約し、最小の構造化応答を返せないほど小さい予算は拒否する。これらは実測から調整する初期値であり、既存exportの全文保持既定を変更しない。
9. 処理可能なrecord内の検索は表示cut前の対象本文を調べ、長文の後半にあるhitも拾う。**入力record上限と表示上限は別**。入力上限超過を処理できない場合はそのraw範囲を`uninspected`として残し、status=partial／rc1を返す。黙って読み捨てて「全件0hit」としない。より大きい上限で原位置から再読できるようにし、cursorが未検査recordを飛ばして完全処理済みと記録しない。切断プレビューを全文と呼ばない。
10. `--event`で再読する場合も表示上限と`text_complete`を明示し、全文表示は利用者の明示操作にする。`--format`による入力形式指定を既存抽出と同じ意味で提供し、autoが判別不能・矛盾を検出したら推測したproviderの0件応答へ隠さない。
11. `--query`は繰返し指定をORとして扱い、各hitに一致したqueryを示す。検索対象はscopeで選ばれた本文（tool scopeでは入力／出力／errorのtext表現）であり、timestampやrole等のmetadata文字列への一致とは分ける。`--regex`を指定した場合だけ正規表現として解釈する。
12. viewの`--limit`は主表示event数、searchの`--limit`はhit数。context／pairを含む**総表示event上限**も別に設定できること。すべてをbyte予算へ算入し、選択数・表示数・未表示数を区別する。結果が存在しても予算で表示しなかったpairは`not_rendered_budget`として、`pending_result`や`unknown_pair`と区別する。未表示eventのID／継続位置を返し、未着と誤認させない。

## 5. event・tool・cursorのデータ契約

### eventの最小情報

| field | 意味 |
|---|---|
| `schema_version`, `record_type` | 新閲覧出力のschema。stdoutはevent／metadata、diagnosticはstderr側に分離。legacy clean schemaへ混ぜない |
| `provider`, `session_id`, `event_id` | provider付きの識別。IDを取得できないfileは明示したfile source IDを使う |
| `timestamp`, `role`, `phase`, `kind`, `source_kind`, `message_origin` | 正規化した意味と元形式を別々に保持。phaseの未知値を捨てない。originはsource上で判別できるhuman／framework／compaction／unknownとし、roleだけでhumanにしない |
| `text`, `text_complete`, `matched_ranges` | 表示本文、その完全性、検索hit位置。byte／文字offsetの単位をschemaで固定する |
| `source` | source ID、入力snapshot境界、raw行（1始まり）、raw byte範囲（0始まり・終端排他）、1 rawから複数eventが出る場合のsubevent index |
| `tool_name`, `call_id`, `paired_event_ids`, `pair_state` | tool関連の識別。元のtool_use_id等も失わず保持し、複数結果・結果未着・対応不明を区別する |

metadataには、filter条件、走査した範囲、取得時の末尾境界、出力件数・byte、既知の意図的除外／未対応／不正／未検査／時刻欠損の区分と件数、切断、`page_cursor`と`append_cursor`、`exhausted`、`status`を持たせる。`exhausted`はsnapshot内の候補走査が末尾へ達した意味であり、`status=partial`なら全本文の検索完了を保証しない。全文hashは任意の監査exportで使用し、日常queryの必須コストにしない。

### provider差と重複

- 新Codexは`response_item/message`から本文を取得できる。terminal mirrorだけでは本文がない場合を検出し、該当profileと欠落数を示す。legacy形式は元eventを根拠に読む。
- 同じ物理発言のAPI／terminal表現は**統合せず、物理行をすべて保持する**（実装決定）。各表現が自分のsource pointerとevent_idを持ち、`mirror_of`と`mirror_relation`で関係を注記する。本文が同じだけの別発言は削除しない。物理行を落とすmergeは行わず、countでpointerを代用しない。
- custom tool入力は不透明なtextでも保持する。JavaScriptやshellを実行・解釈して内側の成功を推測しない。呼出し側の名前と、構造化されている実引数を区別する。
- call/resultはprovider＋session＋call IDの範囲で対応付ける。結果未着、複数結果、結果のみの断片はそのまま示す。近い時刻だからという理由で別callへ結合しない。
- toolのrc、Claudeの外側`is_error`、本文の「実行できなかった」、permission_denialsは異なる観測として保持する。CLIが依頼作業の完了状態を自動決定しない。
- 未対応eventを黙って0件にしない。型ごとのcoverageとraw位置をdiagnosticへ残し、必要なrecordへ戻れるようにする。

### 差分と再現性

- 各queryは開始時のfile末尾を固定し、完全なJSONL行までを対象にする。追記中の未完末尾は待機／pendingとして示し、途中の不正行とは区別する。
- **page cursor**は固定snapshot内の続き。**append cursor**は末尾まで走査した時点の完全行末から、その後の追記を読む位置。途中ページだけでappend cursorを発行せず、未読の過去eventを飛ばさない。
- 入力上限超過等の未検査recordがあれば、そこを処理したことにしてappend cursorを前進させない。拒否位置の再試行cursorを返す。明示的なskipを後から実装する場合は、skip範囲を別記録にして完全性の主張から除く。
- cursorはsource世代、byte／必要なsubevent位置、意味的filter・scope／schema profile、境界anchorを識別する。cursor内のfilterを再利用し、繰返し指定された条件に矛盾があれば拒否する。tail・件数・表示budgetは選択結果の表示条件として分け、tail表示後でも同じfilterで追記を読めるようにする。別filterは先頭から再評価する。
- 1 raw recordが複数eventへ展開される場合、page cursorは**raw行の開始byte＋次のsubevent index**を保存できること。上限がrawの途中に当たっても、次のページで未出力subeventを飛ばさず、既出eventを再送しない。append cursorはそのraw内を全て走査した後の完全行末だけを使う。event本文の表示cutは当該eventの消費と区別し、`event_id`で再読可能にする。
- source置換・切詰め・境界anchor不一致では`cursor_invalid`として停止し、暗黙の先頭再読や別file追従をしない。
- cursorモードはappend-onlyを前提とする。軽量anchorだけで既読prefix全体の不変性を暗号学的に保証したとは言わない。厳密な監査では固定snapshot／hash付きexportを別に使う。
- snapshot境界の固定は、同じinode内の書換えまで防止する機能ではない。読取り中に観測可能な変更を検知した場合は`status: partial`＋`source_changed: true`（rc1）として再試行を要求し、append cursorを発行しない。厳密再現用には保存snapshotを使用する。

## 6. 受入ケース

| ID | 入力・操作 | 合格条件 |
|---|---|---|
| A01 | 宣言した探索root内の開始日が古い更新済session、同cwdの子・別session、長いtitle | fixtureの期待候補集合と一致し、対象外rootを含めた網羅性は主張しない。provider／開始日／最終eventを区別し、titleで予算超過しない |
| A02 | 新Codexのterminal mirror＋API本文、legacy Codex | user／assistant本文とphaseを取得し、欠落や重複の出所を追える |
| A03 | offset付き時刻の境界、欠損時刻、逆行時刻、role指定 | 境界の採否・unknown件数・原文順・tailの対象が仕様どおり |
| A04 | 複数queryとhit、本文末尾のhit、regex誤り、context重複、隣接する内部event | 表示cut前をOR検索し、一致query・hit・contextを区別。不正regexは失敗。role/time外のcontextは明示し、reasoning/developer等のhard scope外を返さない |
| A05 | 並行call、custom形式、Claude tool_use/result、未着結果、予算で未表示の結果、rc0＋拒否本文 | 正しいIDで結び、未着と予算未表示を区別。外側正常終了を依頼完了へ変換しない |
| A06 | 1 rawから複数event、cut・dedup、途中のunknown event | 全出力eventを元byteへ再読できる。未知・切断を明示 |
| A07 | UTF-8、巨大行、1 raw→3eventを含むfile。event上限1でページ送りし、その後複数回追記 | raw途中の全subeventを欠落・重複なく返す。末尾到達後のappend cursorは既読発言を重複せず、完全行の追加eventを落とさない |
| A08 | source置換、切詰め、query変更、anchor不一致 | typed errorと非0終了。暗黙再読・cursor上書きなし |
| A09 | 長いtitle・tool出力・1行巨大JSON・極小予算 | valid JSONLと総量上限を守る。処理上限内の末尾hitを拾い、入力上限超過はuninspected／partial／rc1と再読位置を返す。未検査recordを飛ばすappend cursorは出さない |
| A10 | shell pipe、空結果、途中停止、壊れた入力 | stdoutへ診断を混ぜず、empty／partial／errorを区別する |
| A11 | 既存integration fixture、全出力flag、skill installの選択・skip条件 | legacy互換の差分なし。意図する変更は版と移行手順を別に示す |
| A12 | 途中の不正JSON、末尾の不完全JSON、unknown形式、読取中の変更 | silent successを避け、coverageとpending／invalidの範囲を示す。元入力不変 |

実装時は現行fixtureと人工境界fixtureを先に固定し、対応する失敗を確認してから変更する。巨大入力の性能検証は別枠で、実会話の形を模した匿名fixtureを使い、小fileでの成功を1GB級の証拠へ読み替えない。

## 7. Harness²を踏まえた自己批評と検証計画

参照は[Harness² README](https://github.com/google-research/harness2)のTask-level recursionとHarness components。今回参照したlocal版は`1cfe254e`。考え方を使うことと、native比較実験の完了は区別する。

| 段階 | この要件書で行うこと | 採用判断 |
|---|---|---|
| Propose | A：現行export＋短いskill/helper。B：共通readerにsessions/view/searchを追加 | 便利そうという理由だけでBを全面採用しない |
| Probe | 現行CLIの固定fixture、実会話で繰り返された手作業、別担当のsource監査を照合 | 新CLIは未実装なので、速度・正答率の改善値は出さない |
| Compose | 重複の多い探索・filter・pointer・差分をCLIへ、調査判断・完了解釈はskillへ戻す | 常駐DB・UI・自動要約はMVPから外す。既存exportを維持 |

### 自己批評で確認する3点

1. **viewerが過大になっていないか**：新しい会話管理基盤を作るのではなく、毎回の外部Python処理を減らす。TUI／daemon／vector検索は必要性を別途検証する。
2. **速さと証拠の保持が矛盾していないか**：既定表示をboundedにしながらraw pointerを保持する。差分のたびに全hashを要求しない一方、監査用の固定snapshotを残す。
3. **測定したことと望む結果を混同していないか**：現行の不足を確認しただけでは、新commandの有用性や一般性能を実証したことにならない。変更後の同条件比較を別gateにする。

### 実装後の比較

実例から「会話を発見」「当日の依頼を読む」「生成物の来歴を探す」「call/resultから未実行を見分ける」「追記分を読む」を固定課題にする。baselineは現行skill＋実際に使っていたhelperも使用可とし、不当に弱くしない。

同じsource snapshot、質問、model、tool権限、時間・context予算でA/Bを実行する。一般的な調査手順の変更とprovider固有の変更を別probeにし、実行差を読んでcomposeする。評価基準・採点結果を改善担当へ漏らさず、sourceへ戻れる根拠、誤断定、落としたケース、読取／出力byte、時間、RSSを評価する。

別provider・別repo・情報不足のholdoutで確認し、成功した経験だけをskillへ戻す。同等・悪化・timeoutも残す。決定的fixtureの正誤検査は製品の受入試験であり、Harness²のground-truth-free改善機構そのものの証明とは呼ばない。

## 8. 実装の切り方と未決事項

1. **既存reader／classifierへsource位置とcoverageを通す**：各providerの抜け・parse_errorをfixtureで固定。既存抽出は互換のまま、新query用schemaを分ける。
2. **view/searchを共通queryとして実装する**：filter・context・tail・出力上限・stdoutを先に揃え、保持Vecへ全件溜めない。
3. **sessionsとcursorを接続する**：file指定を基礎に、provider metadata探索を追加する。provider DBはversioned adapterでread-onlyに扱い、未対応schemaを明示する。
4. **skill／配布／計測を更新する**：普段の入口を「一覧→必要箇所→根拠」に短くし、忠実exportは別用途として残す。reference/scriptを配るなら埋込・installer・一時homeでの配布検査も更新する。

実装前に確定する事項：出力／入力record／総表示event上限の初期値、regex仕様、query schemaの版、source世代の軽量識別方法、cursorの渡し方と任意保管場所。異常recordの失敗方針・新commandの終了codeは第4〜5節に従う。候補未選択を曖昧なdefaultで隠さず、実装設計で決めた内容を本書へ戻す。

**初稿時点の完了は要件書のレビューまでだった。** その後 0.3.0 として F01–F10 / N01–N05 を実装し、
§9 のとおり合成 fixture の E2E で局所検証した。native Harness² 比較と global skill への反映は
引き続き未実施（P1 扱い）。初稿の調査証跡は調査側リポジトリの
`temp/agent_session_cli_requirements_20261021/` にローカル証跡として残っているが、
本書と実装の根拠はリポジトリ内の合成 E2E と ADR で自己完結する。

## 9. 実装状況（0.3.0）

### 9.1 F01–F10 の対応

| ID | 実装 | 主な受入テスト |
|---|---|---|
| F01 | `src/query/sessions.rs`, `src/query/source.rs` | `a01_sessions_discovers_exact_cwd_started_old_updated_recent`, `a01_sessions_ambiguous_duplicate_id_fails_rc1`, `a01_sessions_default_roots_resolve_under_isolated_home` |
| F02 | `src/query/normalize.rs`, `src/query/filter.rs` | `a02_provider_bodies_and_phase_and_framework`, `a03_time_boundaries_unknown_retrograde_and_tail`, `f02_private_channel_message_is_hard_scope_excluded` |
| F03 | `src/query/filter.rs`, `src/query/select.rs` | `a04_search_or_queries_context_and_hard_scope`, `f03_context_never_crosses_hard_scope_kind` |
| F04 | `src/query/toolpair.rs` | `a05_tool_pairs_multiple_parallel_custom_claude_pending_and_budget`, `f06_paired_append_renders_absolute_raw_order`, `f_aud02_uninspected_result_is_unknown_pair_not_pending`, `r02_counterpart_total_is_exact_with_truncated_ids`, `cap_preceding_counterpart_does_not_block_append_progress`, `event_lookup_respects_scope_and_privacy_and_pairs`, `viewer_integrity_e2e::f04_pending_call_at_snapshot_ignores_result_appended_later` |
| F05 | `src/query/reader.rs`, `src/query/normalize.rs` | `a06_provenance_subevents_unknown_and_cut`, `f05_source_generation_stable_on_append_changes_on_replace` |
| F06 | `src/query/cursor.rs`, `src/query/runner.rs` | `a07_cursor_pages_through_subevents_then_appends`, `a08_invalid_cursors_are_typed_errors`, `f06_append_between_pages_preserves_old_snapshot_then_append_reads_new`, `f06_search_cursor_reuses_query_and_rejects_conflicting_query`, `f06_cursor_truncation_mid_line_is_truncated`, `f06_cursor_explicit_filters_omitted_vs_explicit`, `f05_source_generation_stable_on_append_changes_on_replace`, `viewer_integrity_e2e::f06_initial_snapshot_holds_while_append_arrives_then_append_reads_it`, `event_absent_only_when_snapshot_fully_inspected`, `f06_cursor_context_profile_is_inherited_and_conflicts_rejected`, `f06_cursor_regex_mode_is_inherited_independently_of_query` |
| F07 | `src/query/runner.rs` | `a09_budgets_record_limit_and_retry`, `f07_retry_cursor_reads_rejected_row_without_prefix`, `f_aud01_max_events_never_drops_hits_without_cursor`, `cap_leading_context_cannot_starve_a_hit` |
| F08 | `src/query/output.rs`, `src/query/runner.rs` | `a10_pipe_empty_error_and_broken_pipe` |
| F09 | `src/cli.rs`, `src/runner.rs` | `a11_legacy_outputs_and_exit_codes_stable`, `a11_installer_matrix_nine_cases`, `a11_version_consistency` |
| F10 | `src/query/reader.rs`, `src/query/detect.rs` | `a12_integrity_bad_middle_pending_tail_and_unknown_format`, `viewer_integrity_e2e::a12_actual_mid_read_mutation_is_detected`, `a12_unchanged_source_scans_completely` |

### 9.2 N01–N05 の証拠と限界

| ID | 証拠 | 限界 |
|---|---|---|
| N01 全件非保持 | `SourceReader` は1行ずつ返し、`Runner` は選択 window のみ保持。`scan.coverage` は type cap（64）＋ overflow count。record 上限は入力段階で適用 | 「入力総量に比例しない」ことはコード構造と限定計測で確認したが、あらゆる provider 形状での証明ではない |
| N02 差分読取り | append cursor は完全行末＋行数＋anchor を保持し、既読 prefix を再読しない（append の read は 579 B / 4 行相当の実測） | 可視範囲外の prefix 書換えは検出しない（append-only 前提） |
| N03 計測分離 | `scan.read_bytes`（logical 主走査）/ `total_read_bytes` と `read_bytes_breakdown` / wall / peak RSS を別掲。`scripts/benchmark_viewer.py` が input sha256・binary sha256・seed・release/debug・compiler・platform を receipt に残す。cold/warm は不明 | 約 69 MB の合成入力のみ。1 GB 級は未検証。速度の一般化はしない |
| N04 無書き込み | query 系は cache/index/state file を作らない。`a12_*` が source の bytes・長さ・mtime 不変と HOME への新規ファイル無しを検査 | atime 不変は保証対象外。provider DB は読まない（OpenCode は明示 root の run JSONL のみ） |
| N05 制御文字・非実行 | text 出力は ESC/OSC/C1 をエスケープ。tool 本文は不透明文字列として保持し実行しない | 端末固有の描画差異までは対象外 |

### 9.3 決定済みの初期値（第4節の暫定案を確定）

- view 既定 50 件 / search 既定 20 hits（`DEFAULT_VIEW_LIMIT` / `DEFAULT_SEARCH_LIMIT`）
- 総表示 event `--max-events` 既定 200、上限 10000
- 出力予算 `--max-output-bytes` 既定 32768、最小 8192（stdout + stderr）
- 入力 record `--max-record-bytes` 既定 1 MiB
- `sessions` の `--scan-limit` 既定 10000
- 意味的 filter のエンコード上限 2048 byte（cursor サイズ有界）
- query schema v1、cursor v1、固定 `source_release_date = 2026-10-21`（0.3.0 時点。0.3.1 は `2026-10-23` で schema/cursor は不変）

### 9.4 未実施・P1

- F11（follow）/ F12（親子・fork 候補）/ F13（stdin・範囲 export・cache）は未実装。
- native Harness² 比較実験は未実施。§7 の discipline（proposal→counterexample→composition）は
  設計レビューと独立監査の進め方として使ったが、比較実験の完了を意味しない。
- CI 通過・merge・tag・release の現在状態は本書に固定しない（ADR 0003 が手順と検証項目を定義し、
  実行状況は GitHub Actions / Releases を参照）。
- 実会話ログでの検証は行っていない。すべて合成 fixture である。
