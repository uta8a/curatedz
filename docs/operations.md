# AWSセットアップと運用

## 現在の検証範囲

ローカルテスト、HTTP/S3モック、CDK synth、Linux ARM64用Lambdaビルドを検証する。
AWS認証・専用bootstrap・GitHub OIDC基盤は準備済み。アプリの実デプロイ・定期実行はGitHub Actionsから確認する。

最初に [bootstrap・GitHub Actions手順](aws-bootstrap.md) を参照する。以下の直接デプロイ手順は管理者による障害復旧用で、通常のデプロイはActionsに任せる。

## 人間が用意するもの

1. 利用するAWSアカウント、リージョン、AWS CLI v2のプロファイル。
2. コンソールログインまたはSSO等の短期認証。アクセスキーやトークンをチャット・リポジトリへ貼らない。
3. 対象アカウント・リージョンへの専用CDK bootstrapとCloudFormationデプロイ権限。権限は管理者と確認する。
4. 実際に収集するテーマ・フィード、実行時刻、データ保持方針、CloudWatchアラームの通知先。

次の環境変数は自分の値を指定する。

```sh
export AWS_PROFILE=YOUR_PROFILE
export AWS_REGION=ap-northeast-1
# コンソールのIAMユーザー/ロールでログインする場合
mise exec -- aws login --profile "$AWS_PROFILE"
# IAM Identity Centerの場合は代わりに初回configureとSSO loginを実行
# mise exec -- aws configure sso --profile "$AWS_PROFILE"
# mise exec -- aws sso login --profile "$AWS_PROFILE"
mise exec -- aws sts get-caller-identity
```

[公式ログイン手順](https://docs.aws.amazon.com/cli/latest/userguide/cli-configure-sign-in.html) を参照。認証情報・パスワードはブラウザや手元のCLIで入力する。

## ビルドとデプロイ

config/topics.yamlを編集し、設定検証とテストを通してからLambdaをビルドする。設定はバイナリに埋め込まれる。

```sh
mise install
mise run prepare:rust
mise exec -- cargo run -p curatedz-cli -- --validate
mise run check
mise run build:lambda
cd infra
mise exec -- npm ci --ignore-scripts --no-audit --no-fund
mise exec -- npm run build
# ACCOUNT_IDはget-caller-identityで確認した自分のAWSアカウントID
mise exec -- npm run cdk -- bootstrap aws://ACCOUNT_ID/ap-northeast-1 \
  --toolkit-stack-name CuratedzToolkit --qualifier curatedz \
  --template bootstrap/toolkit.template.json \
  --cloudformation-execution-policies arn:aws:iam::ACCOUNT_ID:policy/CuratedzCloudFormationExecution \
  --termination-protection
mise exec -- npm run cdk -- diff Curatedz
mise exec -- npm run cdk -- deploy Curatedz
```

リージョンを変える場合はbootstrap先とAWS_REGIONを揃える。
初期デプロイではSchedulerはDISABLED。出力のBucketName、FunctionName、DLQ URL、ErrorAlarmNameを記録する。
`-c synthOnly=true` はテストfixtureをアセットにした構造検証専用。デプロイには使わない。
通常のCDKアプリは実際のLinux ARM64 ELFがない場合に失敗する。

## 自宅同期用IAM Roles Anywhereの準備

CDKは`Curatedz-HomeSync` IAM Role、Roles Anywhere Trust Anchor/Profile、permissions boundary、S3読み取り専用Managed Policyを作成する。IAM UserとAccess Keyは作成しない。CA秘密鍵とworkload秘密鍵はAWS、GitHub Actions、CIログ、Gitへ渡さない。

最初のデプロイ前に、ネットワークへ接続しない管理端末でCAとworkload証明書を作る。以下の出力先はownerだけが読めるGit外のディレクトリとし、実際のパスへ置き換える。

```sh
umask 077
openssl genpkey -algorithm RSA -pkeyopt rsa_keygen_bits:3072 -out HOME_SYNC_CA_DIR/ca-key.pem
openssl req -x509 -new -sha256 -days 3650 -key HOME_SYNC_CA_DIR/ca-key.pem \
  -subj '/CN=curatedz-home-sync-ca' \
  -addext 'basicConstraints=critical,CA:TRUE' \
  -addext 'keyUsage=critical,keyCertSign,cRLSign' \
  -addext 'subjectKeyIdentifier=hash' \
  -out HOME_SYNC_CA_DIR/ca-cert.pem
openssl genpkey -algorithm RSA -pkeyopt rsa_keygen_bits:3072 -out HOME_SYNC_CA_DIR/workload-key.pem
openssl req -new -sha256 -key HOME_SYNC_CA_DIR/workload-key.pem \
  -subj '/CN=curatedz-home-sync' -out HOME_SYNC_CA_DIR/workload.csr
openssl x509 -req -sha256 -days 397 -in HOME_SYNC_CA_DIR/workload.csr \
  -CA HOME_SYNC_CA_DIR/ca-cert.pem -CAkey HOME_SYNC_CA_DIR/ca-key.pem -CAcreateserial \
  -extfile <(printf 'basicConstraints=critical,CA:FALSE\nkeyUsage=critical,digitalSignature\nextendedKeyUsage=clientAuth\n') \
  -out HOME_SYNC_CA_DIR/workload-cert.pem
openssl verify -CAfile HOME_SYNC_CA_DIR/ca-cert.pem HOME_SYNC_CA_DIR/workload-cert.pem
```

Roles AnywhereはTrust AnchorのCA証明書に`basicConstraints = critical, CA:TRUE`を要求する。単に`openssl req -x509`しただけの証明書は、署名検証に成功してもTrust Anchor作成時に`Incorrect basic constraints for CA certificate`で拒否されるため、上記拡張を省略しない。

AWSマネジメントコンソールでParameter Storeを開き、同じリージョンにStandard/Stringの`/curatedz/home-sync/ca-certificate`を作成して、公開情報である`ca-cert.pem`のPEM全文を設定する。SecureStringにはしない。CloudFormationは`AWS::SSM::Parameter::Value<String>`として解決するため、値はGitHub Actionsへ渡らずCIログにも出ない。

bootstrap更新後にCDKをデプロイし、CloudFormationの`Curatedz`スタックで`BucketName`、`SyncRoleArn`、`SyncTrustAnchorArn`、`SyncProfileArn`を確認する。IAM Roleの権限が同期対象S3 prefixの`GetObject`と`ListBucket`だけで、信頼条件にtrust anchor ARN、account、`CN=curatedz-home-sync`があることを確認する。

以前の構成をデプロイ済みの場合、`Curatedz-HomeSync` IAM UserにはRetain policyが設定されているため、スタック更新後も権限を外した状態で残る。Roles Anywhereで同期成功を確認し、Access Keyが存在しないことを確認してから、AWSマネジメントコンソールでこの旧Userを削除する。Roleと同名なので、削除対象の種類がUserであることを必ず確認する。

Podではworkload証明書と秘密鍵をread-only Secretとしてmountし、次の共有AWS configをmountする。ARNとリージョンはCloudFormation出力へ置き換える。

```ini
[default]
region = ap-northeast-1
credential_process = /usr/local/bin/aws_signing_helper credential-process --certificate /var/run/curatedz-aws/tls.crt --private-key /var/run/curatedz-aws/tls.key --trust-anchor-arn TRUST_ANCHOR_ARN --profile-arn PROFILE_ARN --role-arn ROLE_ARN
```

`AWS_CONFIG_FILE`をこのファイルへ向け、`AWS_SDK_LOAD_CONFIG=1`を設定する。Kubernetes Secretには`tls.crt`と`tls.key`だけを投入し、CA秘密鍵、Access Key、AWS session credentialは入れない。証明書の作成・投入コマンドは値がshell historyやCIログへ残らない管理端末上で実行する。

同期プロセスはAWS SDKの`aws_config` targetをWARN以上に制限する。INFOログにはcredential processが返した一時Access Key IDが含まれる場合があるため、この制限を緩和しない。同期確認時もログ本文を表示する前に、Access Key ID、秘密鍵、署名付きURL、Authorization headerのパターンを検査する。

workload証明書は期限前に新しい鍵で再発行し、Secret更新後の同期成功を確認してから旧証明書を失効する。漏えい時に即時失効できるよう、本番利用前にCRLの保管・更新手順を用意する。それまではCA秘密鍵をオフライン保管し、漏えい時はCAとTrust Anchorを交換する。

## 手動確認

CLIをS3へ接続する場合:

```sh
mise exec -- cargo run -p curatedz-cli --features aws -- --bucket YOUR_BUCKET --run-id manual-check
```

リポジトリルートからLambdaを手動起動する場合:

```sh
aws lambda invoke --function-name YOUR_FUNCTION --cli-binary-format raw-in-base64-out \
  --payload '{"schedule_arn":"manual-v1-check","scheduled_time":"2026-11-09T00:00:00Z"}' \
  /tmp/curatedz-lambda-result.json
```

scheduled_timeは実際の確認時刻に置き換える。同じschedule_arn・scheduled_timeは同じrunとして再取得しない。
Lambda結果のFunctionError、status、件数とCloudWatchログを確認する。
S3のstate/v1/head.jsonから公開済みmanifestをたどり、Raw・JSONLを確認する。
同じイベントを再送してgenerationが増えないこと、一部フィード不正時にpartialになること、全件失敗時のDLQとアラームを確認する。
403が出た場合はIAM・バケット名を確認し、権限不足を「データなし」と扱わない。

確認後に定期実行を有効化する:

```sh
cd infra
mise exec -- npm run cdk -- deploy Curatedz -c scheduleEnabled=true
```

毎日07:00 Asia/Tokyoに起動する。時刻変更はinfra/src/stack.tsで行う。
無効化する場合は `npm run cdk -- deploy Curatedz -c scheduleEnabled=false`。
通常のdeployでcontextを省略すると無効に戻るため、有効化後の更新時も明示する。

## 失敗と復旧

- 取得・解析失敗: manifestのsourcesとsource_id付きログを確認する。Rawがあればreplayで解析だけ再実行できる。
- 同じrunの再試行: 保存済みcaptureを使い、headへ既にコミット済みなら元manifestを返す。
- 失敗したHTTPを再取得: 新しいrun_idで収集する。失敗captureは同じrunでは固定される。
- 保存中断: 同じrun_idで再実行する。非公開バッチは取り込まない。
- 索引CAS競合: 最大8回再計画し、上限に達した場合は同じrun_idで再試行する。
- Scheduler配信失敗: DeliveryFailuresキュー。Lambda実行失敗: ExecutionFailuresキュー。
- 部分失敗: FailedSourcesメトリクスとPartialFailureAlarm。全件失敗: Lambda ErrorsとErrorsアラーム。

```sh
mise exec -- cargo run -p curatedz-cli --features aws -- --bucket YOUR_BUCKET --run-id new-replay --replay ORIGINAL_RUN
```

replay時は元runに存在する情報源を指定する。異なるテーマへの関連付けは新しい設定とrunで行い、元の取得時刻・URLで正規化する。

## 保持・費用

S3バケット・ログはRetain。スタック削除後にもデータと料金が残る。
バージョニングされたhead・JSONL、孤立バッチ、Rawの増加を観測し、削除・ライフサイクルは再処理期間を決めてから設定する。
索引16MiBの上限に達する前にDocument数、run数、S3リクエスト量・Lambda時間を計測する。
Lambdaはアカウントの共有同時実行枠を使う。LambdaとCLIの同時実行を含む公開競合はS3 CASで処理する。
アラームは定義のみで通知先は未設定。運用開始時にSNS等への通知設定を別途行う。
## v2 記事処理要求

Collector は RSS batch を head で公開した後、manifest が参照する `article_requests.jsonl` を Article Request Queue へ送信する。
S3 の公開と SQS 送信の間で停止した場合、同じ `schedule_arn` と `scheduled_time` のイベントで Collector Lambda を再実行する。同じ run ID の公開済み manifest が返り、同じ要求が再送される。別の時刻を指定すると別 run になるため、欠落復旧には使わない。

SQS Standard Queue は重複する可能性がある。DLQからの redrive と手動再実行も安全であることは Article Processor の state 条件付き更新で保証する。Article Failure Queue にメッセージがある場合は、URLや本文をログへ出さず message ID と article ID で原因を確認する。

Processorは接続先DNSの全addressとredirect先を検証し、公開HTTP(S)以外を拒否する。robots.txtで禁止された記事、恒久的なHTTP/content-type/抽出失敗はstatusを保存してmessageを成功扱いにする。timeout、接続、429、5xx、読み取り失敗は失敗recordを保存したうえで部分バッチ失敗として再試行し、5回後にDLQへ移動する。外部サイトへのアクセスは予約同時実行数1とorigin単位の1秒間隔で直列化し、robots.txtは実行環境内で1時間cacheする。

v2導入前からS3にあるDocument、または処理version更新後の未処理記事は、AWS認証後に次で確認・投入する。dry-runはURLを表示せず件数だけを返す。実行時はcontent headの成功済みversionを除外し、不変な`article-requests/v1/backfill-<hash>.jsonl`を再送元として保存する。同じコマンドの再実行とSQS重複は安全である。

```sh
mise exec -- cargo run --locked -p curatedz-content-backfill -- \
  --bucket YOUR_BUCKET --queue-url YOUR_ARTICLE_REQUEST_QUEUE --dry-run
mise exec -- cargo run --locked -p curatedz-content-backfill -- \
  --bucket YOUR_BUCKET --queue-url YOUR_ARTICLE_REQUEST_QUEUE
```
