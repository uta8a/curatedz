# curatedz

関心テーマに関連する情報を継続的に収集し、長期的に検索・分析・再処理できるようにする個人向けの情報基盤です。

v1 は **RSS / Atom の収集と保存** に絞ります。テーマと情報源をGit管理し、取得時点のRawと共通スキーマのデータをS3へ保存します。
Web Search、LLM、通知、自宅OpenSearchとの接続は将来対応です。

## 現在の状態

開発指針・エディター設定・CI・設計方針を刷新した段階です。
収集アプリ、ローカルCLI、CDKスタックはまだありません。AWSリソースの作成や自動デプロイも行いません。

## 開発基盤の検証

前提: Git、[mise](https://mise.jdx.dev/) がインストール済みであること。
リポジトリの設定を確認してから、以下を実行します。

```sh
mise trust
mise install
mise run check
```

APIキーやAWS認証情報は不要です。ツールの初回インストールにはネットワーク接続が必要です。
検証対象はJSON/TOML、ローカルのMarkdownリンク、差分の空白、Codexルール、GitHub ActionsのSHA固定、検証ツールの回帰テストです。
アプリのformat・lint・testとCDK synthは、それぞれの実装時に追加します。

## v1 の目標

- YAMLでテーマ・RSS情報源を定義し、多対多の関連を表現する。
- テーマを指定してローカルで収集し、ローカルファイルまたはS3へ出力する。
- Document、Discovery、テーマ関連を分け、同じ記事の重複と新しい発見を区別する。
- Rawを残し、外部APIへ再アクセスせずに正規化を再実行する。
- 情報源ごとの部分失敗、記事更新、再実行に対応する。
- EventBridge Scheduler → 単一Lambdaで定期実行する。

## ドキュメント

- [設計方針と未解決事項](docs/architecture.md)
- [データモデル・保存契約の設計案](docs/data-model.md)
- [開発手順とCI](docs/development.md)
- [運用設計](docs/operations.md)
- [v1実装計画](docs/plan/v1.md)
- [開発者・エージェント向け指針](AGENTS.md)
- [Codex設定の責務](.codex/README.md)

旧RSS → GitHub Discussion構想と旧MVP計画は置き換えました。以前の内容はGit履歴で参照できます。

## ライセンス

[Apache-2.0](LICENSE)
