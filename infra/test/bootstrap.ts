import { test } from 'node:test';
import * as assert from 'node:assert/strict';
import { readFileSync } from 'node:fs';

test('GitHub信頼とCloudFormation権限を専用スコープへ限定する', () => {
  const access = JSON.parse(readFileSync('bootstrap/access.template.json', 'utf8'));
  const toolkit = JSON.parse(readFileSync('bootstrap/toolkit.template.json', 'utf8'));
  const trust = access.Resources.GitHubRole.Properties.AssumeRolePolicyDocument.Statement[0];
  assert.deepEqual(trust.Condition.StringEquals['token.actions.githubusercontent.com:sub'], {
    'Fn::Sub': 'repo:${GitHubRepository}:ref:refs/heads/main',
  });
  assert.equal(trust.Condition.StringEquals['token.actions.githubusercontent.com:aud'], 'sts.amazonaws.com');
  const execution = access.Resources.ExecutionPolicy.Properties.PolicyDocument.Statement;
  const bootstrapVersion = execution.find((entry: { Action: string[] }) => entry.Action.includes('ssm:GetParameters'));
  assert.deepEqual(bootstrapVersion.Resource, [
    { 'Fn::Sub': 'arn:${AWS::Partition}:ssm:${AWS::Region}:${AWS::AccountId}:parameter/cdk-bootstrap/curatedz/version' },
    { 'Fn::Sub': 'arn:${AWS::Partition}:ssm:${AWS::Region}:${AWS::AccountId}:parameter/curatedz/home-sync/ca-certificate' },
  ]);
  const creation = execution.find((entry: { Action: string[] }) => entry.Action.includes('iam:CreateRole'));
  assert.ok(creation.Condition.StringEquals['iam:PermissionsBoundary']);
  const managedPolicy = execution.find((entry: { Action: string[] }) => entry.Action.includes('iam:CreatePolicy'));
  assert.deepEqual(managedPolicy.Action, [
    'iam:CreatePolicy',
    'iam:GetPolicy',
    'iam:DeletePolicy',
    'iam:CreatePolicyVersion',
    'iam:GetPolicyVersion',
    'iam:DeletePolicyVersion',
    'iam:ListPolicyVersions',
    'iam:SetDefaultPolicyVersion',
    'iam:TagPolicy',
    'iam:UntagPolicy',
  ]);
  assert.equal(
    managedPolicy.Resource['Fn::Sub'],
    'arn:${AWS::Partition}:iam::${AWS::AccountId}:policy/Curatedz-*',
  );
  assert.ok(!JSON.stringify(execution).includes('iam:CreateUser'));
  const userManagement = execution.find((entry: { Action: string[] }) => entry.Action.includes('iam:GetUser'));
  assert.ok(!userManagement.Action.includes('iam:CreateAccessKey'));
  const userAttachment = execution.find((entry: { Action: string[] }) => entry.Action.includes('iam:DetachUserPolicy'));
  assert.ok(!userAttachment.Action.includes('iam:AttachUserPolicy'));
  assert.equal(
    userAttachment.Condition.ArnLike['iam:PolicyARN']['Fn::Sub'],
    'arn:${AWS::Partition}:iam::${AWS::AccountId}:policy/Curatedz-*',
  );
  const roleAttachment = execution.find((entry: { Action: string[] }) => entry.Action.includes('iam:AttachRolePolicy'));
  assert.ok(JSON.stringify(roleAttachment.Condition).includes('policy/Curatedz-*'));
  const rolesAnywhere = execution.find((entry: { Action: string[] }) => entry.Action.includes('rolesanywhere:CreateTrustAnchor'));
  assert.ok(rolesAnywhere.Action.includes('rolesanywhere:CreateProfile'));
  assert.ok(JSON.stringify(rolesAnywhere.Resource).includes('rolesanywhere:${AWS::Region}:${AWS::AccountId}:trust-anchor/*'));
  const passRole = execution.find((entry: { Action: string[] }) => entry.Action.includes('iam:PassRole'));
  assert.deepEqual(passRole.Condition.StringEquals['iam:PassedToService'], [
    'lambda.amazonaws.com',
    'scheduler.amazonaws.com',
    'rolesanywhere.amazonaws.com',
  ]);
  const listing = execution.find((entry: { Action: string[] }) => entry.Action.includes('logs:DescribeLogGroups'));
  assert.equal(listing.Resource, '*');
  assert.deepEqual(listing.Condition.StringEquals['aws:RequestedRegion'], { Ref: 'AWS::Region' });
  assert.deepEqual(toolkit.Resources.CloudFormationExecutionRole.Properties.ManagedPolicyArns, {
    Ref: 'CloudFormationExecutionPolicies',
  });
  const deploy = toolkit.Resources.DeploymentActionRole.Properties.Policies[0].PolicyDocument.Statement;
  const write = deploy.find((entry: { Action: string[] }) => entry.Action.includes('cloudformation:CreateStack'));
  assert.equal(write.Resource['Fn::Sub'], 'arn:${AWS::Partition}:cloudformation:${AWS::Region}:${AWS::AccountId}:stack/Curatedz/*');
  assert.ok(!JSON.stringify(access.Resources.GitHubRole).includes('lookup-role'));
  const runtime = access.Resources.RuntimeBoundary.Properties.PolicyDocument.Statement;
  const queue = runtime.find((entry: { Action: string[] }) => entry.Action.includes('sqs:ReceiveMessage'));
  assert.ok(queue.Action.includes('sqs:DeleteMessage'));
  assert.equal(queue.Resource['Fn::Sub'], 'arn:${AWS::Partition}:sqs:${AWS::Region}:${AWS::AccountId}:Curatedz-*');
});
