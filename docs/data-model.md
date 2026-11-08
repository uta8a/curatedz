# データモデル・保存契約の設計案

## 状態

この文書は方針案であり、スキーマ・キー・競合解決方式の確定仕様ではない。
実装時にJSONの具体例、型、ID計算のテストベクトルと共に確定する。旧itemsスキーマとの互換性は前提としない。

## レコードの責務

| 種類 | 主な情報 | 同一性 |
| --- | --- | --- |
| Document | schema_version、document_id、canonical_url、title、published_at、updated_at、authors、summary、content_hash | 正規化URLを基本とする。URLがなければ情報源IDとソース固有ID |
| Discovery | discovery_id、document_id、source_id、source_item_id、observed_at、run_id、raw参照 | 収集実行内の情報源・エントリ単位。同一実行の再試行は同一ID、新しい収集は別の発見 |
| Topic association | document_id、topic_id、根拠となるdiscovery_id、設定バージョン | 記事とテーマの関連およびその根拠。Documentの重複判定から独立する |
| Run manifest | run_id、種別、設定・処理バージョン、出力キー一覧、件数、情報源ごとの結果 | バッチの公開境界。成功、部分成功、全件失敗を区別する |

Documentには情報源固有フィールドを必須にしない。出典はDiscoveryからたどる。
公開日不明はnullとして保持する。Webページ本文はv1では取得しない。
フィードのsummaryに含まれるHTMLは非信頼入力として扱い、将来の表示処理で安全性を確保する。

## IDと更新

Document IDは `sha256:` 接頭辞を持つ決定的なハッシュとする方向。
URLとソースIDフォールバックを識別できる入力形式を使い、区切りやエンコーディングも仕様化する。
タイトル・公開日時・取得日・テーマはIDに含めず、内容更新はcontent_hashで区別する。
同じURLのRSS記事と、将来の検索結果は同じDocumentを参照できるようにする。
URLが変わる同一記事や別URLの転載を自動的に同一視することは保証しない。

URL正規化はscheme/hostの扱い、fragment、既定portなどを規則化し、クエリの一律除去やhttp/httpsの無条件統合は避ける。
ソース固有IDすらない場合の扱いと、内容が異なる同一URLのマージ規則は実装前に決める。

## S3の論理レイアウト案

日付は収集時刻のUTC日付、run_idは再試行で変わらない実行識別子とする方向。
情報源IDは設定で定義した安全なIDを使い、URLを直接キーに埋め込まない。

```text
raw/v1/source=<source_id>/date=<YYYY-MM-DD>/run=<run_id>/response.bin
raw/v1/source=<source_id>/date=<YYYY-MM-DD>/run=<run_id>/metadata.json
normalized/v1/date=<YYYY-MM-DD>/run=<run_id>/documents.jsonl
discovery/v1/date=<YYYY-MM-DD>/run=<run_id>/discoveries.jsonl
associations/v1/date=<YYYY-MM-DD>/run=<run_id>/associations.jsonl
runs/v1/date=<YYYY-MM-DD>/run=<run_id>/manifest.json
```

Rawはレスポンスの元バイト列を保存し、Content-Type、文字コードなど必要な情報はmetadataに残す。
認証ヘッダー、Cookieなど秘密情報をレスポンスメタデータに無差別保存しない。
JSONLは実行単位でまとめる。データ量に応じた分割上限・圧縮形式は実装前に確定する。
manifestには実際に書いたオブジェクトを列挙し、再処理IDと元Rawの参照を記録する。

## 重複排除・コミット

日付を含むキーは整理用であり、グローバルな重複判定用ではない。
HeadObjectだけでは日またぎ重複やチェック後の競合を防げない。

初期案はS3上のDocument ID・内容ハッシュ索引とバッチmanifestを使用する方式。
変更のないDocumentは再出力せず、新しい発見・テーマ関連は保存する。
本文を持たないため、タイトルやsummaryが変わる場合の更新規則も必要になる。

索引を先に更新して出力が失われるケースと、出力後の再試行で重複するケースの両方を扱う。
単一writerと条件付き書き込みなどを比較し、索引・バッチ・manifestの公開順序を決める。
S3には複数オブジェクトを一括でコミットするトランザクションがないため、整合性を仮定だけで済ませない。
索引の全件読み込みは蓄積量に応じて高くなるため、想定データ量と再構築手順も記録する。

## 再処理と取り込み

- 収集済みRawの参照、取得時刻、設定バージョン、正規化バージョンを保持する。
- 同じRawの再処理は新しいネットワークアクセスを行わない。
- 新規収集、同一実行の再試行、正規化バージョン変更による再処理を区別する。
- 将来の取り込み側は完了manifestに列挙されたデータを読み、Document IDでupsertする。
- 更新順序、取り込みカーソル、テーマ関連の削除は確定仕様に含める。

検証には日付をまたぐ同一記事、複数テーマ・情報源、内容更新、部分書き込み、manifest保存失敗を含める。
