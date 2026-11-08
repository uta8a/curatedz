# v1 データ契約

## 設定と実装

Rustの共通ライブラリをCLIとLambdaで共有し、IaCはTypeScript CDKとする。
RSS解析はfeed-rsを使用し、XML宣言の非UTF-8にも対応する。
テーマ・情報源はYAMLで別IDを持ち、未知フィールド、参照切れ、重複、安全でないIDを拒否する。
YAMLの設定内容を正規化JSONにしたSHA-256をconfig_versionとする。

## レコード

- Document: schema_version、document_id、canonical_url、title、published_at、updated_at、authors、summary、content_hash。
- Discovery: discovery_id、document_id、source_id、source_item_id、observed_at、run_id、raw_key。
- Association: document_id、topic_id、discovery_id、config_version。削除せず履歴として蓄積する。
- Manifest: 実行ID、設定・正規化版、収集/replay、元実行ID、情報源ごとの結果、件数、JSONLキー。

Document IDは `sha256:` + SHA256(JSON配列)で、入力はURLありなら `["url", canonical_url]`、なければ `["source", source_id, entry_id]`。
タイトル・公開日時・テーマ・収集日はIDに含めない。内容ハッシュはcontent_hashを空文字にしたDocumentのJSONから計算する。
URLはhttp/httpsだけを許可し、認証情報を拒否する。fragmentを除去し、URLライブラリでhostと既定portを正規化する。クエリを保持し、http/httpsを同一視しない。
公開日時不明はnull。URLもエントリIDもない入力は正規化失敗として記録する。
Discovery IDは `["discovery", run_id, source_id, entry_id, document_id]` のSHA-256。
同一runの再試行は同じ発見、新しいrunは別の発見。重複した同一エントリは実行内で統合する。

## 保存レイアウト

```text
raw/v1/run=<run_id>/source=<source_id>/body=<hash>.bin
raw/v1/run=<run_id>/source=<source_id>/capture.json
batches/v1/<transaction_hash>/documents.jsonl
batches/v1/<transaction_hash>/discoveries.jsonl
batches/v1/<transaction_hash>/associations.jsonl
batches/v1/<transaction_hash>/manifest.json
state/v1/head.json
```

captureは元バイト列への参照、取得時刻、情報源URL、設定版、エラーを記録する。収集失敗もそのrunの結果として固定する。
同じrun_idを違う設定・テーマ選択・replay元で使うことは拒否する。
Rawやバッチは条件付き新規作成し、不変とする。同じキーへの異なる内容は保存エラー。
取得済みcaptureがあれば再取得せず、replayは元runのcapture/bodyだけを読む。
解析失敗のRawも残す。部分成功runの失敗情報源を取得し直すには新しいrun_idで収集する。

## 公開・索引・競合

headはgeneration、Document IDごとの内容ハッシュ・観測時刻・正規化版・情報源ID、run_idごとのmanifest参照と実行契約を持つ。
S3はGetObjectのETagを使い、初回はIf-None-Match、更新はIf-MatchでheadをCAS更新する。
ローカルではOSのファイルロック中に比較し、一時ファイルのrenameで置換する。プロセス終了でロックは解放される。

1. Rawを不変保存する。
2. headを読み、変更Document・Discovery・Associationを計画する。
3. 計画内容のハッシュをtransaction_hashとし、JSONLとmanifestを不変保存する。
4. headのCAS成功で初めてバッチを公開する。索引とrun参照は同じオブジェクト内なので一括更新される。
5. CAS競合時はheadを再読込して計画し直す。有限回で打ち切り、呼び出し側に再試行させる。

取り込み側はheadのrunsにあるmanifestだけを読む。バッチprefixをlistして未公開manifestを取り込まない。
generationはコミット順。manifestのgenerationで順に取り込み、Document IDでupsertする。
CAS直後にプロセスが停止しても、同じrunはheadから完了結果を返す。
書きかけのバッチは非公開のまま残る。自動削除はせず、headから参照されていないことを確認して管理者が削除する。

同じ内容のDocumentは再出力しないが、新しいDiscoveryとAssociationは残す。
更新候補は `(observed_at, normalizer_revision, source_id, content_hash)` の順で比較し、過去Rawのreplayで新しい内容を巻き戻さない。
テーマ関連は履歴であり、設定からテーマを消しても過去の関連は削除しない。

## 初期上限・制約

情報源20件、テーマ100件、各応答2MiB以下、各フィード1000エントリ以下、HTTPタイムアウト10秒以下・試行2回以下。
取得は逐次実行し、情報源を共有するテーマでも取得は一回。明示的にスループットを制限して負荷と実行時間を抑える。
JSONLとheadはそれぞれ16MiBまで。headはDocument索引とrun履歴を全件読み書きするため、長期運用前に計測・分割やコンパクションを検討する。
上限超過は公開せずエラーにする。Rawは残るので設定・実装の調整後に再処理できる。
同じURLの転載検出、変更されたURLの追跡、本文取得、テーマ関連の削除は対象外。

## 公式仕様

- [S3条件付き書き込み](https://docs.aws.amazon.com/AmazonS3/latest/userguide/conditional-writes.html)
- [feed-rsパーサー](https://docs.rs/feed-rs/latest/feed_rs/parser/struct.Parser.html)
- [Schedulerのコンテキスト属性](https://docs.aws.amazon.com/scheduler/latest/UserGuide/managing-schedule-context-attributes.html)

Schedulerのexecution-idは試行ごとに異なるためrun_idに使わず、schedule-arnとscheduled-timeのハッシュを使う。

正規化規則を変更した場合はNORMALIZER_VERSIONを増やす。同じ取得時刻でも新しい正規化版を優先し、過去Rawから新しい取得時刻のDocumentは上書きしない。
