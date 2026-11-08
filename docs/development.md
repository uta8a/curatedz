# 開発手順

## 現在利用できるもの

開発設定とリポジトリ検証のみ。Rustアプリ、収集CLI、CDKは未実装。
以下の手順はAWSや外部APIの認証情報を必要としない。

前提はGitと [mise](https://mise.jdx.dev/) が利用できること。

```sh
mise trust
mise install
mise run check
```

固定ツールはmise.tomlを参照する。検証スクリプトはPython 3.11以上の標準ライブラリで動作する。
Nodeは検証用Codex CLIの導入に使う。Rust・cargo-lambda・CDKのバージョンはアプリ実装時に選定する。
初回ツール取得にはネットワークが必要だが、導入後のリポジトリ検証は外部API・AWSへアクセスしない。
Codexの検証はログイン不要の `execpolicy check` のみを使う。

## ツール更新と14日cooldown

安定版のうち、公開から14日以上経過した最新版を選び、正確なバージョンで固定する。
miseの `minimum_release_age = "14d"` を設定する。
この設定は `latest` などの解決時に適用され、正確な固定バージョンは再判定されないため、更新時にも公開日時を確認する。
仕様は [miseの公式設定リファレンス](https://mise.jdx.dev/configuration/settings.html#minimum_release_age) を参照する。

2026-11-09 JSTに確認した固定値（公開日はUTC）:

| ツール | バージョン | 公開日 |
| --- | --- | --- |
| Python | 3.15.1 | 2026-10-21 |
| rg | 15.2.0 | 2026-07-15 |
| Node | 26.12.1 | 2026-10-21 |
| Codex CLI | 0.164.0 | 2026-10-23 |
| gh | 2.103.2 | 2026-10-23 |
| pinact | 5.0.2 | 2026-10-25 |

候補は `mise latest TOOL --minimum-release-age 14d` で解決する。
公開日時は `mise ls-remote TOOL --json --no-versions-host --strict-metadata` で取得し、npmパッケージは公式レジストリのtimeも確認する。
日時が不明な版をcooldown対象外として採用せず、配布元の情報で確認する。
更新後は `mise install` と `mise run check` を実行し、固定値とこの記録を同じPRで更新する。

## 個別の検証

| コマンド | 対象 |
| --- | --- |
| `mise run check:config` | JSON/TOMLの構文、Markdownのローカルリンク、差分の空白 |
| `mise run check:rules` | Codexルールの読み込みとmatch / not_match例、最終判定 |
| `mise run check:workflows` | GitHub ActionsのSHA固定 |
| `mise run test:tooling` | 検証ツールのシェル解釈・CLIエラー等の回帰テスト |

設定検証はJSON/TOMLのアプリ固有スキーマ検証ではない。
Markdownリンクは単純なインラインのローカルファイルリンクを対象とし、外部URLやアンカーの存在は検証しない。
ルール例はシェルで実行しない。標準出力のJSONを解析し、CLIの異常終了は失敗にする。

## CI

PRとmainへのpushで同じ `mise run check` を実行する。
ActionsはコミットSHAで固定し、権限はcontents: readに限定する。
現段階ではデプロイworkflowやOIDCロールを作らない。

Rust追加時はformat・clippy・単体テスト、CDK追加時は依存ロック・型検査・synthを必須チェックへ追加する。
未実装のアプリ検証をスキップして成功したように見せるジョブは作らない。

## エディターと秘密情報

VS Codeは `.vscode/extensions.json` の拡張を任意に導入する。
.editorconfigとVS Code設定でインデント・改行を統一し、Rust実装時にrust-analyzerで保存時整形する。
Codex設定の責務は [.codex/README.md](../.codex/README.md) を参照する。

.envや出力先output/はGit管理対象外。認証情報やRawの実データをfixtureとして無断で追加しない。
PRは日本語で、変更理由、検証結果、未実施事項、後続作業を記載する。

## アプリ実装時のローカル動作

CLIはテーマ指定、ローカル出力、dry-run、Rawのreplayを提供する予定。
dry-runは永続化を抑止するが、HTTPアクセスまで抑止するとは限らない。
完全にオフラインで検証するfixture経路を別途用意する。
実際のコマンド名とオプションは実装後にREADMEへ記載する。
