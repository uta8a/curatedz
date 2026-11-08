# curatedz

関心テーマを起点にRSS / Atomを収集し、取得時点のRawと正規化データを長期活用するための個人向け基盤です。
Rustの共通処理をローカルCLIとLambdaで使用し、AWSではEventBridge Scheduler → 単一Lambda → S3で実行します。

v1はRSS / Atomに限定します。Web Search、本文抽出、LLM、通知、OpenSearch構築は対象外です。

## ローカルで試す（AWS認証情報・ネット接続不要）

Gitと [mise](https://mise.jdx.dev/) が必要です。初回のツール・依存取得にはネット接続を使います。

```sh
mise trust
mise install
mise exec -- cargo fetch --locked
mise run demo
```

fixtureを読み、output/へRaw・Document・Discovery・テーマ関連・manifest・索引を保存します。
再度 `mise run demo` を実行しても同じrunは再取得・再公開しません。

```sh
# 永続化せずfixtureで確認
mise exec -- cargo run -p curatedz-cli -- --fixtures fixtures --dry-run
# 設定とテーマ参照を検証
mise exec -- cargo run -p curatedz-cli -- --validate
# 実際のフィードを取得してテーマ指定でローカル保存
mise exec -- cargo run -p curatedz-cli -- --topic software-engineering --run-id first-live
# 保存したRawだけを使って再処理（HTTPアクセスなし）
mise exec -- cargo run -p curatedz-cli -- --replay first-live --run-id first-replay
```

設定は [config/topics.yaml](config/topics.yaml)。情報源とテーマは多対多で、共有フィードは一回だけ取得します。
新しい取得には新しいrun_idを指定します。同じrun_idを違う設定・テーマ選択に使うとエラーです。
CLI終了コードは成功0、部分成功2、全件失敗・保存/設定エラー1です。
dry-runは保存を抑止しますが、`--fixtures`なしではHTTPアクセスします。replayとdry-runの同時指定はできません。

## 検証

```sh
mise run check
mise run build:lambda
```

format・clippy・Rustテスト（S3/HTTPモック・CLIを含む）、設定・ルール検証、CDK型検査・構成テスト・synthを実行します。
build:lambdaはLinux ARM64用bootstrapを生成します。単体テストとsynthはAWS認証情報を要求しません。
CIも同じチェックとLambdaビルドを実行します。

## AWS運用

AWS設定・実環境検証は未実施です。[AWSセットアップと運用](docs/operations.md) の手順でアカウント・リージョン・認証を用意します。
Schedulerは初期状態で無効です。手動の収集確認後に有効化します。
S3出力をCLIから使う場合は `cargo run -p curatedz-cli --features aws -- --bucket BUCKET --run-id NAME` を実行します。

## 保存・制約

Document（実体）、Discovery（発見履歴）、Association（テーマ関連）を分離します。
記事のIDは正規化URLを基本とし、タイトル・公開日時・テーマの変更で別記事にしません。
JSONLをバッチ保存後、単一headを条件付き更新して公開するため、日またぎ重複・同時実行・中断再開に対応します。
取り込み側はheadから参照されるmanifestだけを読みます。

初期版は情報源20件、フィード1000エントリ、応答2MiB、各保存オブジェクト16MiBまでです。
索引・実行履歴は全件読み書きするため、長期運用ではサイズと費用の計測・コンパクションが必要です。

## ドキュメント

- [アーキテクチャ](docs/architecture.md)
- [データ契約と整合性](docs/data-model.md)
- [開発手順](docs/development.md)
- [AWSセットアップ・運用](docs/operations.md)
- [v1実装状況](docs/plan/v1.md)
- [開発指針](AGENTS.md)
- [Codex設定](.codex/README.md)

## ライセンス

[Apache-2.0](LICENSE)
