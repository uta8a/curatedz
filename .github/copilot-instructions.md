# curatedz の開発

開発者への応答とドキュメントは日本語で記述する。

リポジトリ共通の開発指針は [AGENTS.md](../AGENTS.md) を正とする。
設計は [docs/architecture.md](../docs/architecture.md)、作業範囲は [docs/plan/v1.md](../docs/plan/v1.md) を参照する。

v1 は関心テーマを起点とした RSS / Atom の収集・正規化・S3保存・再処理を対象とする。
Web Search、通知、LLM、OpenSearch構築は将来対応である。

現在は開発基盤の整備段階であり、アプリ・CLI・CDKスタックは未実装。
実装予定のコマンドを動作確認済みの手順として記載しない。
