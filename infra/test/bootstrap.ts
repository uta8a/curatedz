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
  const creation = execution.find((entry: { Action: string[] }) => entry.Action.includes('iam:CreateRole'));
  assert.ok(creation.Condition.StringEquals['iam:PermissionsBoundary']);
  assert.deepEqual(toolkit.Resources.CloudFormationExecutionRole.Properties.ManagedPolicyArns, {
    Ref: 'CloudFormationExecutionPolicies',
  });
  const deploy = toolkit.Resources.DeploymentActionRole.Properties.Policies[0].PolicyDocument.Statement;
  const write = deploy.find((entry: { Action: string[] }) => entry.Action.includes('cloudformation:CreateStack'));
  assert.equal(write.Resource['Fn::Sub'], 'arn:${AWS::Partition}:cloudformation:${AWS::Region}:${AWS::AccountId}:stack/Curatedz/*');
  assert.ok(!JSON.stringify(access.Resources.GitHubRole).includes('lookup-role'));
});
