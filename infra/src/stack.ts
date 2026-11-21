import * as cdk from 'aws-cdk-lib';
import { Construct } from 'constructs';
import * as s3 from 'aws-cdk-lib/aws-s3';
import * as lambda from 'aws-cdk-lib/aws-lambda';
import * as lambdaEvents from 'aws-cdk-lib/aws-lambda-event-sources';
import * as logs from 'aws-cdk-lib/aws-logs';
import * as iam from 'aws-cdk-lib/aws-iam';
import * as scheduler from 'aws-cdk-lib/aws-scheduler';
import * as sqs from 'aws-cdk-lib/aws-sqs';
import * as cloudwatch from 'aws-cdk-lib/aws-cloudwatch';
import * as rolesanywhere from 'aws-cdk-lib/aws-rolesanywhere';

export interface CollectorProps extends cdk.StackProps {
  collectorCode: lambda.Code;
  processorCode: lambda.Code;
  scheduleEnabled: boolean;
}

export class CollectorStack extends cdk.Stack {
  constructor(scope: Construct, id: string, props: CollectorProps) {
    super(scope, id, props);
    iam.PermissionsBoundary.of(this).apply(
      iam.ManagedPolicy.fromManagedPolicyName(this, 'RuntimeBoundary', 'CuratedzRuntimeBoundary'),
    );
    const bucket = new s3.Bucket(this, 'Archive', {
      blockPublicAccess: s3.BlockPublicAccess.BLOCK_ALL,
      encryption: s3.BucketEncryption.S3_MANAGED,
      enforceSSL: true,
      versioned: true,
      removalPolicy: cdk.RemovalPolicy.RETAIN,
    });
    const syncReadPolicy = new iam.ManagedPolicy(this, 'SyncReadPolicy', {
      description: 'Read-only access for the home Kubernetes OpenSearch sync worker',
      statements: [
        new iam.PolicyStatement({
          actions: ['s3:GetObject'],
          resources: [
            bucket.arnForObjects('state/v1/*'),
            bucket.arnForObjects('batches/v1/*'),
            bucket.arnForObjects('article-content/v1/*'),
          ],
        }),
        new iam.PolicyStatement({
          actions: ['s3:ListBucket'],
          resources: [bucket.bucketArn],
          conditions: { StringLike: { 's3:prefix': ['state/v1/*', 'batches/v1/*', 'article-content/v1/*'] } },
        }),
      ],
    });
    const homeSyncCaCertificate = new cdk.CfnParameter(this, 'HomeSyncCaCertificate', {
      type: 'AWS::SSM::Parameter::Value<String>',
      default: '/curatedz/home-sync/ca-certificate',
      description: 'SSM String parameter containing the PEM-encoded home sync CA certificate',
    });
    const trustAnchor = new rolesanywhere.CfnTrustAnchor(this, 'SyncTrustAnchor', {
      name: 'Curatedz-HomeSync',
      enabled: true,
      source: {
        sourceType: 'CERTIFICATE_BUNDLE',
        sourceData: { x509CertificateData: homeSyncCaCertificate.valueAsString },
      },
      tags: [{ key: 'Project', value: 'curatedz' }],
    });
    const syncRole = new iam.Role(this, 'SyncRole', {
      roleName: 'Curatedz-HomeSync',
      description: 'Short-lived S3 read access for the home Kubernetes sync worker',
      assumedBy: new iam.ServicePrincipal('rolesanywhere.amazonaws.com', {
        conditions: {
          StringEquals: {
            'aws:SourceAccount': this.account,
            'aws:PrincipalTag/x509Subject/CN': 'curatedz-home-sync',
          },
          ArnEquals: { 'aws:SourceArn': trustAnchor.attrTrustAnchorArn },
        },
      }),
      maxSessionDuration: cdk.Duration.hours(1),
    });
    syncRole.assumeRolePolicy?.addStatements(new iam.PolicyStatement({
      principals: [new iam.ServicePrincipal('rolesanywhere.amazonaws.com')],
      actions: ['sts:TagSession', 'sts:SetSourceIdentity'],
      conditions: {
        StringEquals: {
          'aws:SourceAccount': this.account,
          'aws:PrincipalTag/x509Subject/CN': 'curatedz-home-sync',
        },
        ArnEquals: { 'aws:SourceArn': trustAnchor.attrTrustAnchorArn },
      },
    }));
    syncReadPolicy.attachToRole(syncRole);
    const syncProfile = new rolesanywhere.CfnProfile(this, 'SyncProfile', {
      name: 'Curatedz-HomeSync',
      enabled: true,
      durationSeconds: 3600,
      roleArns: [syncRole.roleArn],
      tags: [{ key: 'Project', value: 'curatedz' }],
    });
    const logGroup = new logs.LogGroup(this, 'CollectorLogs', {
      retention: logs.RetentionDays.ONE_MONTH,
      removalPolicy: cdk.RemovalPolicy.RETAIN,
    });
    const articleFailures = new sqs.Queue(this, 'ArticleFailures', {
      retentionPeriod: cdk.Duration.days(14),
      encryption: sqs.QueueEncryption.SQS_MANAGED,
      enforceSSL: true,
    });
    const articleRequests = new sqs.Queue(this, 'ArticleRequests', {
      visibilityTimeout: cdk.Duration.minutes(6),
      retentionPeriod: cdk.Duration.days(14),
      encryption: sqs.QueueEncryption.SQS_MANAGED,
      enforceSSL: true,
      deadLetterQueue: { queue: articleFailures, maxReceiveCount: 5 },
    });
    const collector = new lambda.Function(this, 'Collector', {
      runtime: lambda.Runtime.PROVIDED_AL2023,
      architecture: lambda.Architecture.ARM_64,
      handler: 'bootstrap',
      code: props.collectorCode,
      memorySize: 1024,
      timeout: cdk.Duration.minutes(15),
      logGroup,
      environment: {
        CURATEDZ_BUCKET: bucket.bucketName,
        CURATEDZ_ARTICLE_QUEUE_URL: articleRequests.queueUrl,
      },
    });
    articleRequests.grantSendMessages(collector);
    const processorLogs = new logs.LogGroup(this, 'ArticleProcessorLogs', {
      retention: logs.RetentionDays.ONE_MONTH,
      removalPolicy: cdk.RemovalPolicy.RETAIN,
    });
    const processor = new lambda.Function(this, 'ArticleProcessor', {
      runtime: lambda.Runtime.PROVIDED_AL2023,
      architecture: lambda.Architecture.ARM_64,
      handler: 'bootstrap',
      code: props.processorCode,
      memorySize: 1024,
      timeout: cdk.Duration.minutes(1),
      logGroup: processorLogs,
      environment: { CURATEDZ_BUCKET: bucket.bucketName },
    });
    processor.addEventSource(new lambdaEvents.SqsEventSource(articleRequests, {
      batchSize: 5,
      reportBatchItemFailures: true,
      maxConcurrency: 2,
    }));
    for (const prefix of ['batches/v1/', 'article-requests/v1/', 'article-content/v1/']) {
      processor.addToRolePolicy(new iam.PolicyStatement({
        actions: ['s3:GetObject', 's3:PutObject'],
        resources: [bucket.arnForObjects(`${prefix}*`)],
      }));
    }
    processor.addToRolePolicy(new iam.PolicyStatement({
      actions: ['s3:ListBucket'], resources: [bucket.bucketArn],
      conditions: { StringLike: { 's3:prefix': ['batches/v1/*', 'article-requests/v1/*', 'article-content/v1/*'] } },
    }));
    for (const prefix of ['raw/v1/', 'batches/v1/', 'state/v1/']) {
      collector.addToRolePolicy(new iam.PolicyStatement({
        actions: ['s3:GetObject', 's3:PutObject'],
        resources: [bucket.arnForObjects(`${prefix}*`)],
      }));
    }
    // 存在しないキーのGetObjectを403ではなく404として判別するための権限。
    collector.addToRolePolicy(new iam.PolicyStatement({
      actions: ['s3:ListBucket'], resources: [bucket.bucketArn],
      conditions: { StringLike: { 's3:prefix': ['raw/v1/*', 'batches/v1/*', 'state/v1/*'] } },
    }));
    const deliveryFailures = new sqs.Queue(this, 'DeliveryFailures', {
      retentionPeriod: cdk.Duration.days(14),
      encryption: sqs.QueueEncryption.SQS_MANAGED,
      enforceSSL: true,
    });
    collector.configureAsyncInvoke({
      retryAttempts: 0,
      maxEventAge: cdk.Duration.hours(1),
      // スケジュール配信失敗はScheduler DLQ、関数の実行失敗はLambda DLQ。
    });
    const failures = new sqs.Queue(this, 'ExecutionFailures', {
      retentionPeriod: cdk.Duration.days(14),
      encryption: sqs.QueueEncryption.SQS_MANAGED,
      enforceSSL: true,
    });
    const cfnFunction = collector.node.defaultChild as lambda.CfnFunction;
    cfnFunction.deadLetterConfig = { targetArn: failures.queueArn };
    failures.grantSendMessages(collector);

    const group = new scheduler.CfnScheduleGroup(this, 'ScheduleGroup');
    const role = new iam.Role(this, 'SchedulerRole', {
      assumedBy: new iam.ServicePrincipal('scheduler.amazonaws.com', {
        conditions: {
          StringEquals: { 'aws:SourceAccount': this.account },
          ArnEquals: { 'aws:SourceArn': group.attrArn },
        },
      }),
    });
    collector.grantInvoke(role);
    deliveryFailures.grantSendMessages(role);
    new scheduler.CfnSchedule(this, 'Schedule', {
      groupName: group.ref,
      flexibleTimeWindow: { mode: 'OFF' },
      scheduleExpression: 'cron(0 7 * * ? *)',
      scheduleExpressionTimezone: 'Asia/Tokyo',
      state: props.scheduleEnabled ? 'ENABLED' : 'DISABLED',
      target: {
        arn: collector.functionArn, roleArn: role.roleArn,
        input: JSON.stringify({ schedule_arn: '<aws.scheduler.schedule-arn>', scheduled_time: '<aws.scheduler.scheduled-time>' }),
        retryPolicy: { maximumEventAgeInSeconds: 3600, maximumRetryAttempts: 2 },
        deadLetterConfig: { arn: deliveryFailures.queueArn },
      },
    });
    const errorAlarm = new cloudwatch.Alarm(this, 'Errors', {
      metric: collector.metricErrors({ period: cdk.Duration.minutes(5) }),
      threshold: 1, evaluationPeriods: 1,
      treatMissingData: cloudwatch.TreatMissingData.NOT_BREACHING,
    });
    const partialFilter = new logs.MetricFilter(this, 'PartialFailures', {
      logGroup,
      filterPattern: logs.FilterPattern.literal('{ $.fields.failed_sources > 0 }'),
      metricNamespace: 'Curatedz', metricName: 'FailedSources',
      metricValue: '$.fields.failed_sources', defaultValue: 0,
    });
    new cloudwatch.Alarm(this, 'PartialFailureAlarm', {
      metric: partialFilter.metric({ period: cdk.Duration.days(1), statistic: 'Sum' }),
      threshold: 1, evaluationPeriods: 1,
      treatMissingData: cloudwatch.TreatMissingData.NOT_BREACHING,
    });
    new cloudwatch.Alarm(this, 'ArticleFailureAlarm', {
      metric: articleFailures.metricApproximateNumberOfMessagesVisible({
        period: cdk.Duration.minutes(5),
      }),
      threshold: 1,
      evaluationPeriods: 1,
      treatMissingData: cloudwatch.TreatMissingData.NOT_BREACHING,
    });
    new cloudwatch.Alarm(this, 'ArticleBacklogAlarm', {
      metric: articleRequests.metricApproximateNumberOfMessagesVisible({
        period: cdk.Duration.minutes(5),
      }),
      threshold: 100,
      evaluationPeriods: 2,
      treatMissingData: cloudwatch.TreatMissingData.NOT_BREACHING,
    });
    new cloudwatch.Alarm(this, 'ArticleOldestMessageAlarm', {
      metric: articleRequests.metricApproximateAgeOfOldestMessage({
        period: cdk.Duration.minutes(5),
      }),
      threshold: cdk.Duration.hours(1).toSeconds(),
      evaluationPeriods: 1,
      treatMissingData: cloudwatch.TreatMissingData.NOT_BREACHING,
    });
    new cdk.CfnOutput(this, 'BucketName', { value: bucket.bucketName });
    new cdk.CfnOutput(this, 'FunctionName', { value: collector.functionName });
    new cdk.CfnOutput(this, 'ErrorAlarmName', { value: errorAlarm.alarmName });
    new cdk.CfnOutput(this, 'ExecutionFailureQueue', { value: failures.queueUrl });
    new cdk.CfnOutput(this, 'DeliveryFailureQueue', { value: deliveryFailures.queueUrl });
    new cdk.CfnOutput(this, 'ArticleRequestQueue', { value: articleRequests.queueUrl });
    new cdk.CfnOutput(this, 'ArticleFailureQueue', { value: articleFailures.queueUrl });
    new cdk.CfnOutput(this, 'SyncReadPolicyArn', { value: syncReadPolicy.managedPolicyArn });
    new cdk.CfnOutput(this, 'SyncRoleArn', { value: syncRole.roleArn });
    new cdk.CfnOutput(this, 'SyncTrustAnchorArn', { value: trustAnchor.attrTrustAnchorArn });
    new cdk.CfnOutput(this, 'SyncProfileArn', { value: syncProfile.attrProfileArn });
  }
}
