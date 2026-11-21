import { test } from 'node:test';
import * as assert from 'node:assert/strict';
import * as cdk from 'aws-cdk-lib';
import * as lambda from 'aws-cdk-lib/aws-lambda';
import { Template, Match } from 'aws-cdk-lib/assertions';
import { CollectorStack } from '../src/stack';

test('非公開保存、定期実行、DLQ、最小権限を構成する', () => {
  const app = new cdk.App();
  const template = Template.fromStack(new CollectorStack(app, 'Test', {
    collectorCode: lambda.Code.fromAsset('../fixtures'),
    processorCode: lambda.Code.fromAsset('../fixtures'),
    scheduleEnabled: false,
  }));
  template.resourceCountIs('AWS::Lambda::Function', 2);
  template.allResourcesProperties('AWS::IAM::Role', {
    PermissionsBoundary: Match.anyValue(),
  });
  template.hasResourceProperties('AWS::Lambda::Function', {
    Runtime: 'provided.al2023', Architectures: ['arm64'], Timeout: 900,
    ReservedConcurrentExecutions: Match.absent(),
    DeadLetterConfig: { TargetArn: Match.anyValue() },
  });
  template.hasResourceProperties('AWS::Lambda::EventSourceMapping', {
    BatchSize: 5,
    FunctionResponseTypes: ['ReportBatchItemFailures'],
  });
  template.hasResourceProperties('AWS::Lambda::Function', {
    ReservedConcurrentExecutions: 1,
    Timeout: 60,
  });
  template.hasResourceProperties('AWS::Lambda::Function', {
    Environment: { Variables: Match.objectLike({
      CURATEDZ_BUCKET: Match.anyValue(), CURATEDZ_ARTICLE_QUEUE_URL: Match.anyValue(),
    }) },
  });
  template.hasResource('AWS::S3::Bucket', { DeletionPolicy: 'Retain' });
  template.hasResourceProperties('AWS::S3::Bucket', {
    VersioningConfiguration: { Status: 'Enabled' },
    PublicAccessBlockConfiguration: {
      BlockPublicAcls: true, BlockPublicPolicy: true, IgnorePublicAcls: true, RestrictPublicBuckets: true,
    },
  });
  template.hasResourceProperties('AWS::Scheduler::Schedule', {
    State: 'DISABLED', ScheduleExpressionTimezone: 'Asia/Tokyo',
    Target: Match.objectLike({ Input: '{"schedule_arn":"<aws.scheduler.schedule-arn>","scheduled_time":"<aws.scheduler.scheduled-time>"}' }),
  });
  template.resourceCountIs('AWS::SQS::Queue', 4);
  template.hasResourceProperties('AWS::SQS::Queue', {
    VisibilityTimeout: 360, MessageRetentionPeriod: 1209600,
    RedrivePolicy: Match.anyValue(), SqsManagedSseEnabled: true,
  });
  const policies = JSON.stringify(template.findResources('AWS::IAM::Policy'));
  assert.ok(!policies.includes('s3:*'));
  assert.ok(!policies.includes('s3:DeleteObject'));
  assert.ok(!policies.includes('secretsmanager:'));
  assert.ok(policies.includes('sqs:SendMessage'));
  assert.ok(policies.includes('s3:prefix'));
  assert.ok(policies.includes('article-requests/v1/*'));
  const managed = JSON.stringify(template.findResources('AWS::IAM::ManagedPolicy'));
  assert.ok(managed.includes('article-content/v1/'));
  assert.ok(managed.includes('s3:GetObject'));
  assert.ok(!managed.includes('s3:PutObject'));
  template.resourceCountIs('AWS::IAM::User', 0);
  template.hasResourceProperties('AWS::IAM::Role', {
    RoleName: 'Curatedz-HomeSync',
    PermissionsBoundary: Match.anyValue(),
    MaxSessionDuration: 3600,
  });
  template.hasResourceProperties('AWS::IAM::ManagedPolicy', {
    Roles: [Match.anyValue()],
  });
  template.resourceCountIs('AWS::IAM::AccessKey', 0);
  template.hasResourceProperties('AWS::RolesAnywhere::TrustAnchor', {
    Name: 'Curatedz-HomeSync', Enabled: true,
    Source: {
      SourceType: 'CERTIFICATE_BUNDLE',
      SourceData: { X509CertificateData: { Ref: 'HomeSyncCaCertificate' } },
    },
  });
  template.hasResourceProperties('AWS::RolesAnywhere::Profile', {
    Name: 'Curatedz-HomeSync', Enabled: true, DurationSeconds: 3600,
    RoleArns: [Match.anyValue()],
  });
  const roles = JSON.stringify(template.findResources('AWS::IAM::Role'));
  assert.ok(roles.includes('rolesanywhere.amazonaws.com'));
  assert.ok(roles.includes('aws:SourceArn'));
  assert.ok(roles.includes('aws:SourceAccount'));
  assert.ok(roles.includes('aws:PrincipalTag/x509Subject/CN'));
  const alarms = JSON.stringify(template.findResources('AWS::CloudWatch::Alarm'));
  assert.ok(alarms.includes('ApproximateNumberOfMessagesVisible'));
  assert.ok(alarms.includes('ApproximateAgeOfOldestMessage'));
});
