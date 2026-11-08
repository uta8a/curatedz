# Codex の開発設定

プロジェクトの要件と作業範囲は [AGENTS.md](../AGENTS.md) を参照する。

## 設定の責務

- `config.toml` は workspace-write のみ指定する。
- モデル、承認方法、ネットワーク接続は利用者・管理環境の設定を継承する。
- `rules/` は読み取り用コマンドの限定的な許可と、システム操作の禁止を定義する。
- GitHub への書き込みやデプロイを一律に自動許可しない。実行可否はユーザーの依頼と実行環境のポリシーに従う。
- ルールは argv の先頭に一致する。オプションの位置を変えたコマンドを含め、汎用的なアクセス制御の代替にはならない。
- `.codex/` の編集が保護される実行環境では、その環境の承認機構を使う。

## Skills

既存の6つの skill は、CI確認、Issue操作、PR作成、コミット整形、ルール編集という開発支援用なので維持する。
アプリの仕様は skill に重複記載せず、AGENTS.md と docs に置く。

## 検証

```sh
mise run check:rules
```

ルール例の文字列はシェルで実行しない。Codex CLI に引数として渡し、判定と読み込み時の例の検証を確認する。
CLI の非ゼロ終了や不正なJSONは失敗として扱う。

## 参照

- [公式ルール仕様](https://learn.chatgpt.com/docs/agent-configuration/rules)
- [公式設定リファレンス](https://learn.chatgpt.com/docs/config-file/config-reference)

検証用の CLI バージョンは mise.toml に固定する。更新時は `mise run check` を通す。
