import * as cdk from 'aws-cdk-lib';
import { Construct } from 'constructs';
import * as s3 from 'aws-cdk-lib/aws-s3';
import * as lambda from 'aws-cdk-lib/aws-lambda';
import * as logs from 'aws-cdk-lib/aws-logs';
import * as iam from 'aws-cdk-lib/aws-iam';
import * as scheduler from 'aws-cdk-lib/aws-scheduler';
import * as sqs from 'aws-cdk-lib/aws-sqs';
import * as cloudwatch from 'aws-cdk-lib/aws-cloudwatch';

export interface CollectorProps extends cdk.StackProps {
  code: lambda.Code;
  scheduleEnabled: boolean;
}

export class CollectorStack extends cdk.Stack {
  constructor(scope: Construct, id: string, props: CollectorProps) {
    super(scope, id, props);
    const bucket = new s3.Bucket(this, 'Archive', {
      blockPublicAccess: s3.BlockPublicAccess.BLOCK_ALL,
      encryption: s3.BucketEncryption.S3_MANAGED,
      enforceSSL: true,
      versioned: true,
      removalPolicy: cdk.RemovalPolicy.RETAIN,
    });
    const logGroup = new logs.LogGroup(this, 'CollectorLogs', {
      retention: logs.RetentionDays.ONE_MONTH,
      removalPolicy: cdk.RemovalPolicy.RETAIN,
    });
    const collector = new lambda.Function(this, 'Collector', {
      runtime: lambda.Runtime.PROVIDED_AL2023,
      architecture: lambda.Architecture.ARM_64,
      handler: 'bootstrap',
      code: props.code,
      memorySize: 1024,
      timeout: cdk.Duration.minutes(15),
      reservedConcurrentExecutions: 1,
      logGroup,
      environment: { CURATEDZ_BUCKET: bucket.bucketName },
    });
    for (const prefix of ['raw/v1/', 'batches/v1/', 'state/v1/']) {
      collector.addToRolePolicy(new iam.PolicyStatement({
        actions: ['s3:GetObject', 's3:PutObject'],
        resources: [bucket.arnForObjects(`${prefix}*`)],
      }));
    }
    // 存在しないキーのGetObjectを403ではなく404として判別するための権限。
    collector.addToRolePolicy(new iam.PolicyStatement({
      actions: ['s3:ListBucket'], resources: [bucket.bucketArn],
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
    new cdk.CfnOutput(this, 'BucketName', { value: bucket.bucketName });
    new cdk.CfnOutput(this, 'FunctionName', { value: collector.functionName });
    new cdk.CfnOutput(this, 'ErrorAlarmName', { value: errorAlarm.alarmName });
    new cdk.CfnOutput(this, 'ExecutionFailureQueue', { value: failures.queueUrl });
    new cdk.CfnOutput(this, 'DeliveryFailureQueue', { value: deliveryFailures.queueUrl });
  }
}
