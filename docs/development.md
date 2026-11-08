# 開発手順

## 開発環境

Gitとmiseをインストールし、リポジトリの設定を確認する。

```sh
mise trust
mise install
mise run prepare:rust
mise exec -- cargo fetch --locked
mise run demo
mise run check
mise run build:lambda
```

初回ツール・依存取得はネット接続が必要。テストは外部API・AWS認証情報なしで実行する。
テスト内のHTTP/S3サーバーはlocalhostへ接続するため、ローカルネットワーク接続を許可する。
全体テストのワークスペースはS3 SDKもビルドする。手早いローカル開発は `cargo test -p curatedz-core -p curatedz-cli`。

アプリはRust 1.99.1、cargo-lambda 1.9.2、Zig 0.15.2でLambdaへクロスビルドする。
Zigはcargo-lambdaとの互換を確認した0.15系列に固定する。
CDKはinfra/package.json、RustはCargo.tomlとそれぞれのロックファイルでバージョンを固定する。

## ツール更新と14日cooldown

安定版のうち、公開から14日以上経過した最新版を選び、正確なバージョンで固定する。
miseの `minimum_release_age = "14d"` を設定する。
この設定は `latest` などの解決時に適用され、正確な固定バージョンは再判定されないため、更新時にも公開日時を確認する。
仕様は [miseの公式設定リファレンス](https://mise.jdx.dev/configuration/settings.html#minimum_release_age) を参照する。

2026-11-09 JSTに確認した固定値（公開日はUTC）:

| ツール | バージョン | 公開日 |
| --- | --- | --- |
| Rust | 1.99.1 | 2026-10-15 |
| cargo-lambda | 1.9.2 | 2026-08-21 |
| Zig（互換系列） | 0.15.2 | 2025-10-11 |
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
| `mise run check:rust` | format、clippy、全featureのRustテスト |
| `mise run check:infra` | npm ci、TypeScript strict、CDK構成テスト、認証不要のsynth |
| `mise run build:lambda` | Linux ARM64用bootstrapビルド |
| `mise run demo` | fixtureからローカル出力 |

設定検証はJSON/TOMLのアプリ固有スキーマ検証ではない。
Markdownリンクは単純なインラインのローカルファイルリンクを対象とし、外部URLやアンカーの存在は検証しない。
ルール例はシェルで実行しない。標準出力のJSONを解析し、CLIの異常終了は失敗にする。

## CI

PRとmainへのpushで同じ `mise run check` を実行する。
ActionsはコミットSHAで固定し、権限はcontents: readに限定する。
AWS設定は後で行うため、デプロイworkflowやOIDCロールは作らない。

Rustはfmt・clippy・全featureのテスト、CDKは型検査・構成テスト・synthを必須チェックに含める。
CIはLinux ARM64のLambda実アセットもビルドする。AWSへのデプロイは行わない。

## エディターと秘密情報

VS Codeは `.vscode/extensions.json` の拡張を任意に導入する。
.editorconfigとVS Code設定でインデント・改行を統一し、Rust実装時にrust-analyzerで保存時整形する。
Codex設定の責務は [.codex/README.md](../.codex/README.md) を参照する。

.envや出力先output/はGit管理対象外。認証情報やRawの実データをfixtureとして無断で追加しない。
PRは日本語で、変更理由、検証結果、未実施事項、後続作業を記載する。

## CLIと検証データ

実行例は [README](../README.md) を参照。
fixtures/topics.yamlとfixture RSSは外部アクセスなしのテスト用。config/topics.yamlは実運用向けの設定例。
`--fixtures fixtures` は各情報源IDの `<id>.xml` を読む。URLは正規化のbase URIとして利用する。
CLIはstdoutにmanifest、stderrに構造化ログを出す。成功0、部分成功2、全件失敗1。
単体テストは一時ディレクトリまたはMemoryStoreを使用し、実行後にGit管理データを残さない。

## 再現性

Cargoは `--locked`、npmは `npm ci --ignore-scripts` をCIで使う。
依存更新時はロックファイルを更新し、Rust・CDK・Lambdaビルドまで再確認する。
config/schema.jsonはエディター向けで、ID重複やテーマの参照整合性などの正確な検証はRustのConfig::validateが行う。
