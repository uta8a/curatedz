# 計画: bootstrap 以外を GitHub Actions CI でデプロイするための IaC 拡張

対象計画:
- `docs/plan/2026-02-09-mvp-rss-to-s3-to-github-discussion.md`
- `docs/plan/2026-02-09-mvp-issue-breakdown.md`

## 目的
- 手元実行は `cdk bootstrap` のみに限定する
- 以降のデプロイ（`cdk synth/deploy`）は GitHub Actions から実行する
- 可能な限り設定をコード管理し、手作業を最小化する

## 必要なもの（チェックリスト）

### A. ローカルで一度だけ実施するもの
- AWS アカウント/リージョンの確定（dev/prod を分ける場合は両方）
- CDK bootstrap 実行
  - 可能なら bootstrap テンプレートをリポジトリ管理し、`--template` で適用
  - 例: `cdk bootstrap aws://<account-id>/<region>`

### B. GitHub 側の準備
- GitHub Actions workflow ファイル
  - `ci.yml`（lint/test/synth）
  - `deploy.yml`（OIDC で AWS AssumeRole → `cdk deploy`）
- Environment（例: `dev`, `prod`）と保護ルール
  - 必要なら手動承認を必須化
- Repository Variables / Secrets（最小）
  - `AWS_ACCOUNT_ID`
  - `AWS_REGION`
  - `AWS_ROLE_TO_ASSUME`（OIDC 先 Role ARN）
  - `CDK_APP_CMD`（必要なら）

### C. AWS 側の準備（GitHub OIDC）
- IAM OIDC Provider: `token.actions.githubusercontent.com`
- GitHub Actions 用 IAM Role（例: `GitHubActionsCdkDeployRole`）
  - 信頼ポリシー条件:
    - `aud == sts.amazonaws.com`
    - `sub == repo:<org>/<repo>:ref:refs/heads/<branch>` など
  - 権限:
    - 最小は `cdk deploy` 実行に必要な権限
    - 運用初期は管理しやすさ優先で広め、後で絞る運用でも可
- CloudTrail / CloudWatch Logs で AssumeRole 監査可能にする

### D. リポジトリで IaC 化するもの
- `cdk.json` / `bin/` / `lib/` の CDK エントリポイント
- bootstrap 設定値（qualifier, toolkit stack 名）をドキュメント化
- IAM ポリシーのコード化（Lambda 実行ロール、State Machine ロール等）
- SSM Parameter / Secrets 参照名の固定化（値の投入手順も Runbook 化）

## 既存 Issue への差し込みタスク

### CI-1（#1 の直後に追加）
- タイトル案: `デプロイ戦略を固定する（local bootstrap + CI deploy）`
- 目的:
  - どのブランチ/環境で自動デプロイするかを先に固定し、IAM 条件に反映する
- 受け入れ条件:
  - 対象ブランチ、対象環境、ロール命名、リージョンが文書化されている

### CI-2（#2 の前に追加）
- タイトル案: `CDK bootstrap方式を確定しテンプレート化する`
- 目的:
  - bootstrap を再現可能にする（手元実行はこの手順だけ）
- スコープ:
  - `cdk bootstrap` コマンドとオプション（必要なら `--template`）を確定
  - 実行結果（Toolkit スタック名、qualifier）を記録
- 受け入れ条件:
  - 新規環境で同一手順により bootstrap できる

### CI-3（#2 の直後に追加）
- タイトル案: `GitHub OIDC Roleを準備する`
- 目的:
  - GitHub Actions から長期鍵なしで AWS にデプロイ可能にする
- スコープ:
  - OIDC Provider / IAM Role / Trust policy / Permission policy
  - `sub` 条件をリポジトリ・ブランチ・Environment に制限
- 受け入れ条件:
  - GitHub Actions で `aws sts get-caller-identity` が成功する

### CI-4（#2 の直後に追加、CI-3 依存）
- タイトル案: `GitHub ActionsのCI workflowを実装する`
- 目的:
  - PR 時に品質ゲート（lint/test/synth）を実施する
- 受け入れ条件:
  - PR で workflow が実行され、失敗時は merge できない設定が可能

### CI-5（#7 の前に追加、CI-3/CI-4 依存）
- タイトル案: `GitHub Actionsのdeploy workflowを実装する`
- 目的:
  - main 反映時に AWS へ自動デプロイする
- スコープ:
  - OIDC AssumeRole
  - `cdk deploy --require-approval never`
  - 対象 stack の明示（誤デプロイ防止）
- 受け入れ条件:
  - main へのマージで自動デプロイされる
  - 実行ログからどのコミットがどのスタックを更新したか追跡できる

### CI-6（#8 の直後に追加）
- タイトル案: `環境保護とデプロイガードレールを設定する`
- 目的:
  - 誤操作防止と監査性を確保する
- スコープ:
  - GitHub Environment 承認
  - production への branch 制限
  - デプロイ失敗時の通知（最低限）
- 受け入れ条件:
  - 保護ルール違反時に deploy が実行されない

### CI-7（#11 と同時に追加）
- タイトル案: `CI/CD Runbookを整備する`
- 目的:
  - OIDC 失敗、権限不足、bootstrap 不整合の復旧手順を明文化する
- 受け入れ条件:
  - `docs/` に運用手順があり、新規メンバーが再現できる

## 改訂後の推奨実行順
1. #1（既存）
2. CI-1
3. CI-2
4. #2（既存）
5. CI-3
6. CI-4
7. #3, #4, #5, #6（既存）
8. CI-5
9. #7（既存）
10. #8（既存）
11. CI-6
12. #9, #10（既存）
13. #11（既存）
14. CI-7

## 補足（現実的な例外）
- 「bootstrap 以外は CI で実行」を厳密に守る場合でも、OIDC Role の初回作成は事前準備が必要
- 可能な限り IaC 化するため、OIDC 関連の trust/policy JSON と設定値はリポジトリ管理し、変更履歴を残す
