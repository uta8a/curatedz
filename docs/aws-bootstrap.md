# AWS bootstrap・GitHub Actionsデプロイ手順

この文書のアカウントID `123456789012`、`YOUR_LOGIN/YOUR_REPOSITORY` はダミー。実際の値に置き換える。
パスワード、アクセスキー、セッショントークンはこの文書・Git・チャットに記録しない。

## 準備した構成

- `CuratedzDeploymentAccess`: GitHub OIDC provider、GitHub用ロール、CloudFormation実行ポリシー、アプリ実行ロールのpermissions boundary。
- `CuratedzToolkit`: qualifier `curatedz` の専用CDK bootstrap。S3/ECR、公開・デプロイ・実行・lookupロール、SSMのbootstrap版を作成する。
- 既存の共通 `CDKToolkit`（qualifier `hnb659fds`）は変更しない。
- GitHub用ロールの信頼は指定リポジトリの `main` と `aud=sts.amazonaws.com` に限定する。PRや別ブランチは認証できない。
- GitHub用ロールは専用のファイル公開・デプロイロールだけを引き受ける。lookup/image公開ロールへの権限は付けない。
- デプロイロールは `Curatedz` スタックの直接作成・更新用。CloudFormationのAdministratorAccessフォールバックは削除する。
- アプリのIAMロール作成には `CuratedzRuntimeBoundary` が必須。boundary自体やGitHub用ロールをアプリの実行権限から変更することはできない。

公開対象・権限は [access.template.json](../infra/bootstrap/access.template.json)、bootstrapは [toolkit.template.json](../infra/bootstrap/toolkit.template.json) を参照する。
両方の基盤スタックには終了保護を設定する。共有され得るOIDC providerはRetainとし、スタック削除で他リポジトリの認証を壊さない。通常のstock `cdk bootstrap` で上書きせず、必ずこのテンプレートを使用する。

## 1. ローカルツールとログイン

```sh
mise trust
mise install
mise run prepare:rust
mise exec -- npm --prefix infra ci --ignore-scripts --no-audit --no-fund
mise run build:lambda

export AWS_PROFILE=curatedz
export AWS_REGION=ap-northeast-1
# ダミー値。確認済みの自分のアカウントIDへ置き換える
export EXPECTED_AWS_ACCOUNT_ID=123456789012
mise exec -- aws login --profile "$AWS_PROFILE"
mise exec -- aws sts get-caller-identity --profile "$AWS_PROFILE"
```

IAM Identity Centerを利用する場合は `aws configure sso` と `aws sso login` を使う。
返されたAccountとEXPECTED_AWS_ACCOUNT_IDを目視確認する。スクリプトでも不一致を拒否する。
初回のみbootstrap用IAM/CloudFormation/S3/ECR/SSMの作成権限が必要。既存の管理用プロファイルを使用し、長期アクセスキーは発行しない。
GitHub CLIは対象リポジトリの管理権限を持つアカウントでログインしておく。

## 2. bootstrapを実行

リポジトリルートから実行する。

```sh
mise exec -- python3 scripts/bootstrap.py --repository YOUR_LOGIN/YOUR_REPOSITORY
```

スクリプトは次の処理を行う。

1. STSでログイン先と期待するアカウントIDを比較する。
2. 新規環境では既存GitHub OIDC providerを確認し、なければ作成する。再実行時は既存スタックのCreateOidcProvider値を維持し、管理中のproviderを誤削除しない。
3. CuratedzDeploymentAccessをCloudFormationで作成・更新する。
4. 専用テンプレートと実行ポリシーでCuratedzToolkitをbootstrapする。
5. GitHubリポジトリ変数 `AWS_ACCOUNT_ID`、`AWS_REGION`、`AWS_DEPLOY_ROLE_ARN` を設定する。認証情報はGitHubへ保存しない。

相当するbootstrapコマンド（ARN中のIDもダミー）:

```sh
cd infra
mise exec -- npm run cdk -- bootstrap aws://123456789012/ap-northeast-1 \
  --profile curatedz \
  --toolkit-stack-name CuratedzToolkit \
  --qualifier curatedz \
  --template bootstrap/toolkit.template.json \
  --cloudformation-execution-policies arn:aws:iam::123456789012:policy/CuratedzCloudFormationExecution \
  --termination-protection
```

専用CloudFormation実行ポリシーの更新権限はcuratedz名のリソースに限定する。CloudWatch LogsのDescribeLogGroupsはリソース単位の指定に非対応のため、対象リージョンの一覧取得だけを例外として許可する。スタック名や物理名を変更する場合はポリシーも見直す。
bootstrap基盤そのものの変更は管理用プロファイルで行い、GitHub Actionsに基盤管理権限を追加しない。

## 3. 確認

```sh
mise exec -- aws cloudformation describe-stacks --stack-name CuratedzDeploymentAccess --profile "$AWS_PROFILE" --region "$AWS_REGION" --query 'Stacks[].StackStatus'
mise exec -- aws cloudformation describe-stacks --stack-name CuratedzToolkit --profile "$AWS_PROFILE" --region "$AWS_REGION" --query 'Stacks[].{Status:StackStatus,TerminationProtection:EnableTerminationProtection}'
mise exec -- aws ssm get-parameter --name /cdk-bootstrap/curatedz/version --profile "$AWS_PROFILE" --region "$AWS_REGION" --query Parameter.Value
mise exec -- gh variable list --repo YOUR_LOGIN/YOUR_REPOSITORY
```

CloudFormationのCREATE_COMPLETEまたはUPDATE_COMPLETE、専用bootstrapの終了保護、bootstrap版、GitHub変数3つを確認する。
再実行は通常「変更なし」になる。既存スタックが失敗状態ならeventsを確認してから復旧し、他プロジェクトのbootstrapを削除しない。

## 4. GitHub Actionsからデプロイ

[deploy.yaml](../.github/workflows/deploy.yaml) はmainのCI成功を受け、検証したSHAをcheckoutしてテスト・Lambdaビルドを行い、OIDCで認証して `Curatedz` を更新する。
mainに存在しないworkflowは実行されないため、この変更をマージ後に初回デプロイする。mainからのworkflow_dispatchにも対応する。
別ブランチからの手動実行はjob条件とOIDC信頼の両方で拒否する。

`CURATEDZ_SCHEDULE_ENABLED` は未設定時false。初回はSchedulerを無効でデプロイし、[運用手順](operations.md) のLambda手動確認後にtrueへ変更してmainからDeployを再実行する。
デプロイは `--method direct` を使う。専用デプロイロールは変更セット作成や他スタック更新を許可しない。
デプロイworkflowは直列化し、開始時にmainの先頭SHAを確認して古いCIからの遅延デプロイを拒否する。

AWSデプロイ用アクセスキーやシークレットは不要。OIDCによるGitHubからの実認証とアプリの実デプロイは確認済みである。

## 実施記録（2026-11-09 JST）

対象の実アカウントではOIDC基盤と専用bootstrapを作成した。アカウントIDと認証情報は記録しない。
共通CDKToolkitは保持し、アプリCuratedzのデプロイはGitHub Actionsへ委ねる。
両スタックは作成・更新完了状態となり、終了保護を有効化し、スクリプト再実行では変更なしを確認した。
IAMシミュレーションで、専用deploy roleのみの引き受け、boundary付きroleのみの作成、他スタック更新の拒否を確認した。
GitHubの変数3つも設定済み。最終状態・再実行結果はPRの検証欄も参照する。

## IAM Roles Anywhere移行記録（2026-11-21 JST）

自宅Kubernetes同期を長期Access KeyからIAM Roles Anywhereへ移行するため、`CuratedzDeploymentAccess`を更新した。CloudFormation実行Roleには、`Curatedz-*` Role/Policy、Roles AnywhereのTrust Anchor/Profile、公開CA証明書用Parameterだけを操作する権限を追加した。`iam:PassRole`は既存のLambda/Schedulerに加え、`rolesanywhere.amazonaws.com`だけを許可する。IAM UserとAccess Keyの作成権限は付与しない。

外部CAとworkload証明書はGit外で作成し、秘密鍵はAWSやCIへ渡さず、公開CA証明書だけを`/curatedz/home-sync/ca-certificate`へ登録した。初回Trust Anchor作成ではCA証明書の`basicConstraints = critical, CA:TRUE`が必須であり、省略時は`Incorrect basic constraints for CA certificate`で拒否された。修正済み公開証明書をParameter Store version 2として登録し、workload証明書を再検証した。

Roles Anywhere初回利用に必要な`AWSServiceRoleForRolesAnywhere`は、CloudFormation実行Roleへ汎用の`iam:CreateServiceLinkedRole`を追加せず、管理者が`rolesanywhere.amazonaws.com`を指定して一度だけ作成した。アプリstackの最終デプロイ結果とKubernetes側の同期確認は、完了後に運用記録へ追記する。

同日の実デプロイでは、SQSから記事処理LambdaへのEvent Source Mapping作成時に、CloudFormation実行Roleの`lambda:CreateEventSourceMapping`不足を検出した。関数ARN用の`lambda:*`だけではEvent Source Mapping ARNを対象にできないため、Mappingの作成・参照・更新・削除を専用ARNへ追加した。stackは`UPDATE_ROLLBACK_COMPLETE`まで復旧しており、修正後の再デプロイ結果は完了後に追記する。

## 実環境デプロイ完了記録（2026-11-22 JST）

Event Source Mapping権限を含む`CuratedzDeploymentAccess`を管理用プロファイルで更新し、専用toolkitは変更なしであることを確認した。その後、main commit `6098ace`のCI、Runtime images、Deployがすべて成功し、`Curatedz` stackは`UPDATE_COMPLETE`となった。

実環境ではRoles AnywhereのTrust Anchor、Profile、`Curatedz-HomeSync` Role、S3読み取りPolicyが作成済みである。SQSから記事処理LambdaへのEvent Source Mappingは`Enabled`、batch size 5、maximum concurrency 2であり、Lambdaのreserved concurrencyは設定していない。旧方式の`Curatedz-HomeSync` IAM Userが存在しないことも確認した。

その後、自宅Kubernetesへworkload証明書を配置し、CronJobがRoles Anywhere認証でS3の公開済みRSS・本文データを差分取得してOpenSearchへ投入できることを確認した。検索UI/APIはCloudflare Tunnel / Access配下で利用できる状態である。

## テンプレート保守

専用toolkitはaws-cdk CLI 2.1146.1の公式bootstrapを元に、DeploymentActionRoleとCloudFormationExecutionRoleを制限したもの。
CDK更新時は標準テンプレートとの差分を確認し、bootstrap版と権限制約を維持してから管理用プロファイルで更新する。
YAMLのJSON化には既存のRust依存を使用できる。

```sh
mise exec -- cargo run --quiet -p curatedz-core --example yaml_to_json -- infra/node_modules/aws-cdk/lib/api/bootstrap/bootstrap-template.yaml > /tmp/bootstrap-base.json
```

JSON化だけでは権限制限は入らない。生成結果をそのまま専用bootstrapへ適用しない。

## 公式資料

- [CDK bootstrapのカスタマイズ](https://docs.aws.amazon.com/cdk/v2/guide/bootstrapping-customizing.html)
- [GitHub OIDCとAWS](https://docs.github.com/en/actions/how-tos/secure-your-work/security-harden-deployments/oidc-in-aws)
- [AWS CLIログイン](https://docs.aws.amazon.com/cli/latest/userguide/cli-configure-sign-in.html)

- [CloudWatch Logsのアクション・リソース制約](https://docs.aws.amazon.com/service-authorization/latest/reference/list_logs.html)
