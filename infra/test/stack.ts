import { test } from 'node:test';
import * as assert from 'node:assert/strict';
import * as cdk from 'aws-cdk-lib';
import * as lambda from 'aws-cdk-lib/aws-lambda';
import { Template, Match } from 'aws-cdk-lib/assertions';
import { CollectorStack } from '../src/stack';

test('非公開保存、定期実行、DLQ、最小権限を構成する', () => {
  const app = new cdk.App();
  const template = Template.fromStack(new CollectorStack(app, 'Test', {
    code: lambda.Code.fromAsset('../fixtures'), scheduleEnabled: false,
  }));
  template.resourceCountIs('AWS::Lambda::Function', 1);
  template.allResourcesProperties('AWS::IAM::Role', {
    PermissionsBoundary: Match.anyValue(),
  });
  template.hasResourceProperties('AWS::Lambda::Function', {
    Runtime: 'provided.al2023', Architectures: ['arm64'], Timeout: 900, ReservedConcurrentExecutions: 1,
    DeadLetterConfig: { TargetArn: Match.anyValue() },
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
  template.resourceCountIs('AWS::SQS::Queue', 2);
  const policies = JSON.stringify(template.findResources('AWS::IAM::Policy'));
  assert.ok(!policies.includes('s3:*'));
  assert.ok(!policies.includes('s3:DeleteObject'));
  assert.ok(!policies.includes('secretsmanager:'));
});
