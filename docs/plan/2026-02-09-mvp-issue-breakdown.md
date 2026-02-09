# MVP 実装計画 Issue 分解（RSS -> S3 -> Discussion）

元計画:
- `docs/plan/2026-02-09-mvp-rss-to-s3-to-github-discussion.md`
- `docs/plan/2026-02-09-iac-cicd-bootstrap-and-github-actions.md`

## 方針
- まずは 1 feed の縦切りを完了させる
- 依存関係を明示して、並行可能なものは並行で進める
- 各 Issue は「Done が判定できる受け入れ条件」を持つ
- デプロイは「ローカルで `cdk bootstrap` のみ」、それ以外は GitHub Actions（OIDC）で実行する

## Issue 一覧

### 1. 基本設計の固定（I/O, 命名, 設定スキーマ）
- タイトル案: `MVPのI/O仕様と設定スキーマを確定する`
- 目的:
  - 実装着手前に、S3 key / schema v1 / SSM JSON / Secrets 名を固定する
- スコープ:
  - 正規化 schema v1 フィールド定義
  - `item_id` 生成ルール（`sha256:` prefix を含む）
  - S3 key 規約（raw/items/digest/feedback）
  - SSM パラメータ JSON 形式
  - Secrets キー名（GitHub token, HMAC secret）
- 受け入れ条件:
  - `docs/architecture.md` か `docs/plan/` に確定仕様が追記されている
  - 実装側で参照するキー名が一意に決まっている
- 依存:
  - なし

### CI-1. デプロイ戦略の固定（local bootstrap + CI deploy）
- タイトル案: `デプロイ戦略を固定する（local bootstrap + CI deploy）`
- 目的:
  - ブランチ/環境ごとのデプロイルールを先に決め、IAM Trust 条件へ反映する
- スコープ:
  - 対象ブランチ（例: `main`）の定義
  - GitHub Environment（例: `dev`, `prod`）の定義
  - アカウント/リージョン、ロール命名規則の固定
- 受け入れ条件:
  - 方針が `docs/plan/` か `docs/architecture.md` に明記されている
  - OIDC trust policy に必要な条件（`sub`）が定義済み
- 依存:
  - #1

### CI-2. CDK bootstrap 手順の確定とテンプレート化
- タイトル案: `CDK bootstrap方式を確定しテンプレート化する`
- 目的:
  - ローカル実行を bootstrap のみに限定し、再現可能にする
- スコープ:
  - `cdk bootstrap` コマンド/オプションの確定
  - 必要に応じて bootstrap テンプレート管理（`--template`）
  - Toolkit stack 名や qualifier の記録
- 受け入れ条件:
  - 新規環境で同手順により bootstrap 可能
  - 手順がドキュメント化されている
- 依存:
  - CI-1

### 2. CDK 土台（Stack 構成 + 最小 IAM）
- タイトル案: `CDKでMVP基盤リソースを作成する`
- 目的:
  - MVP 実装のための最小 AWS リソースを定義する
- スコープ:
  - S3 バケット
  - Step Functions State Machine
  - Rust Lambda 用の関数定義（雛形）
  - EventBridge Scheduler
  - IAM 最小権限（S3 prefix 限定、SSM、Secrets）
- 受け入れ条件:
  - `cdk synth` が通る
  - Lambda 実行ロールに不要権限が含まれていない
- 依存:
  - #1
  - CI-2

### CI-3. GitHub OIDC Provider/Role 準備
- タイトル案: `GitHub OIDC Roleを準備する`
- 目的:
  - GitHub Actions から長期鍵なしで AWS AssumeRole できるようにする
- スコープ:
  - IAM OIDC Provider（`token.actions.githubusercontent.com`）
  - デプロイ用 IAM Role（例: `GitHubActionsCdkDeployRole`）
  - Trust policy 条件（`aud=sts.amazonaws.com`, `sub=repo:<org>/<repo>:...`）
  - `cdk deploy` 実行に必要な権限ポリシー
- 受け入れ条件:
  - GitHub Actions 上で `aws sts get-caller-identity` が成功する
  - 想定外ブランチから AssumeRole できない
- 依存:
  - CI-1
  - CI-2

### CI-4. GitHub Actions CI workflow（lint/test/synth）
- タイトル案: `GitHub ActionsのCI workflowを実装する`
- 目的:
  - PR 時に品質ゲートを実行する
- スコープ:
  - `.github/workflows/ci.yml`
  - `lint` / `test` / `cdk synth` 実行
  - branch protection で必須チェック化できる状態
- 受け入れ条件:
  - PR で workflow が自動実行される
  - 失敗時に merge できない設定にできる
- 依存:
  - #2
  - CI-3

### 3. Rust Lambda: fetch-sources（取得 + raw 保存）
- タイトル案: `fetch-sources Lambdaを実装しraw RSSをS3保存する`
- 目的:
  - SSM の feed URL から RSS/Atom を取得し raw を保存する
- スコープ:
  - HTTP fetch（タイムアウト/リトライ）
  - S3 key: `raw/v1/date=YYYY-MM-DD/feed=<escaped>/ts=<iso>.xml`
  - 構造化ログ（`feed_url`, `stage`）
- 受け入れ条件:
  - 1 feed の raw XML が S3 に保存される
  - 失敗時にエラーが握りつぶされず、原因がログで追跡できる
- 依存:
  - #1
  - #2

### 4. Rust Lambda: parse-normalize（パース + 正規化 + item 冪等保存）
- タイトル案: `parse-normalize Lambdaを実装してitems v1を保存する`
- 目的:
  - raw から正規化 item を生成し、重複をスキップして保存する
- スコープ:
  - RSS 2.0 / Atom 最低限対応
  - `item_id = sha256(canonical_url_or_link + title + published_at_iso)` 実装
  - S3 `HeadObject` による重複スキップ
  - S3 key: `items/v1/date=YYYY-MM-DD/<item_id>.json`
- 受け入れ条件:
  - 同一入力で同一 `item_id` が生成される
  - 既存 item がある場合に保存をスキップできる
- 依存:
  - #1
  - #2
  - #3

### 5. Rust Lambda: build-digest（digest 生成 + 保存）
- タイトル案: `build-digest Lambdaでdigest.jsonとdiscussion.mdを生成する`
- 目的:
  - items から digest artifacts を生成して保存する
- スコープ:
  - アイテム集約/並び順
  - markdown テンプレートの安定化
  - S3 key:
    - `digest/v1/date=YYYY-MM-DD/run=<iso>/digest.json`
    - `digest/v1/date=YYYY-MM-DD/run=<iso>/discussion.md`
  - `HeadObject` による重複スキップ
- 受け入れ条件:
  - digest.json と discussion.md が S3 に保存される
  - 同一 run の再実行で重複作成しない
- 依存:
  - #1
  - #2
  - #4

### 6. Rust Lambda: post-discussion（GitHub GraphQL 投稿）
- タイトル案: `post-discussion LambdaでGitHub Discussionを投稿する`
- 目的:
  - digest markdown を GitHub Discussion に投稿する
- スコープ:
  - Secrets Manager からトークン取得
  - GraphQL mutation 実装
  - 投稿済みマーカー保存（`posted.json`）
  - 本文要件（@mention + フォームリンク）
- 受け入れ条件:
  - 指定リポジトリに Discussion が投稿される
  - 同一 run で再実行時は投稿済みマーカーによりスキップされる
- 依存:
  - #1
  - #2
  - #5

### CI-5. GitHub Actions deploy workflow（OIDC + cdk deploy）
- タイトル案: `GitHub Actionsのdeploy workflowを実装する`
- 目的:
  - `main` 反映時に CI から自動デプロイする
- スコープ:
  - `.github/workflows/deploy.yml`
  - OIDC AssumeRole
  - `cdk deploy --require-approval never`
  - デプロイ対象 stack 明示
- 受け入れ条件:
  - `main` マージで deploy が実行される
  - 実行ログからコミットと更新スタックを追跡できる
- 依存:
  - CI-3
  - CI-4

### 7. Step Functions オーケストレーション（MVP縦切り）
- タイトル案: `State Machineを実装しMVPの縦切りを接続する`
- 目的:
  - `fetch -> parse -> build -> post` を実行可能にする
- スコープ:
  - 状態遷移定義
  - retry/catch の基本方針
  - 入出力（run_id など）の受け渡し
- 受け入れ条件:
  - 手動実行で end-to-end が完了する
  - 失敗時にどの段階で落ちたか追跡できる
- 依存:
  - #2
  - #3
  - #4
  - #5
  - #6
  - CI-5

### 8. EventBridge Scheduler 接続（定期実行）
- タイトル案: `SchedulerからState Machineを定期起動する`
- 目的:
  - 定期バッチとして自動実行できるようにする
- スコープ:
  - cron/rate 設定
  - Scheduler 実行ロール
  - 最低限の運用パラメータ化（有効化/無効化、時刻）
- 受け入れ条件:
  - スケジュール時刻に State Machine が起動する
  - 実行ログから定期トリガーであることが確認できる
- 依存:
  - #7

### CI-6. 環境保護とデプロイガードレール
- タイトル案: `環境保護とデプロイガードレールを設定する`
- 目的:
  - 誤操作防止と監査性を確保する
- スコープ:
  - GitHub Environment 承認ルール
  - production への branch 制限
  - デプロイ失敗通知（最低限）
- 受け入れ条件:
  - 保護ルール違反時に deploy が実行されない
- 依存:
  - CI-5
  - #8

### 9. Rust Lambda(Function URL): form-handler（GET/POST + HMAC検証）
- タイトル案: `form-handler Lambda(Function URL)を実装する`
- 目的:
  - フィードバックフォームの表示と保存を実現する
- スコープ:
  - GET: digest を読み取り動的 HTML 生成
  - POST: HMAC + exp 検証、S3 保存
  - S3 key: `feedback/v1/digest_id=<digest_id>/submitted_at=<iso>-<rand>.json`
- 受け入れ条件:
  - 正常な署名付き URL でフォーム表示/送信できる
  - 期限切れ/改ざんトークンは 4xx で拒否される
- 依存:
  - #1
  - #2
  - #5

### 10. テスト整備（fixtures + 純粋ロジック）
- タイトル案: `RSS/Atom fixtureとユニットテストを整備する`
- 目的:
  - 主要ロジックの回帰防止
- スコープ:
  - fixture: RSS2.0 / Atom / 日付欠落 / entities
  - 正規化ロジック単体テスト
  - `item_id` 安定性テスト
  - 冪等スキップ分岐テスト（AWS依存を薄く）
- 受け入れ条件:
  - `cargo test` が通る
  - 主要ロジックの期待動作がテストで明文化されている
- 依存:
  - #4
  - #5

### 11. E2E 検証と運用ドキュメント
- タイトル案: `MVPのE2E検証手順と運用Runbookを作成する`
- 目的:
  - デプロイ後の検証と運用を再現可能にする
- スコープ:
  - 実行手順（手動起動、定期実行確認）
  - 失敗時の切り分けポイント（Lambda/State Machine/S3）
  - 既知制約（1 feed 前提、PAT 運用など）
- 受け入れ条件:
  - 新規メンバーがドキュメントだけで検証を再実行できる
  - DoD 1-7 の確認結果が記録される
- 依存:
  - #8
  - #10

### CI-7. CI/CD Runbook 整備
- タイトル案: `CI/CD Runbookを整備する`
- 目的:
  - OIDC 失敗、権限不足、bootstrap 不整合の復旧手順を明文化する
- スコープ:
  - `docs/` に復旧手順を追加
  - 監査時の確認観点（AssumeRole, deploy 履歴）を定義
- 受け入れ条件:
  - 新規メンバーが CI/CD トラブルを手順だけで再現・切り分けできる
- 依存:
  - #11
  - CI-6

## 実行順（推奨）
1. #1
2. CI-1
3. CI-2
4. #2
5. CI-3
6. CI-4
7. #3, #4, #5, #6
8. CI-5
9. #7
10. #8
11. CI-6
12. #9, #10
13. #11
14. CI-7

## MVP 完了判定（チェックリスト）
- [ ] 1 feed 取り込み
- [ ] raw RSS を S3 保存
- [ ] 正規化 items(v1) を S3 保存
- [ ] digest(JSON/Markdown) を S3 保存
- [ ] GitHub Discussion 投稿（@mention + form link）
- [ ] 冪等スキップ（items/digest/discussion）
- [ ] 将来の部分成功拡張を阻害しない設計

## CI/CD 完了判定（チェックリスト）
- [ ] ローカル操作が `cdk bootstrap` のみに限定されている
- [ ] GitHub Actions OIDC で AWS AssumeRole できる
- [ ] CI（lint/test/synth）が PR で必須化されている
- [ ] deploy が GitHub Actions 経由で実行される
- [ ] Environment 保護と監査導線がある
