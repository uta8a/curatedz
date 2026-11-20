import * as cdk from 'aws-cdk-lib';
import * as lambda from 'aws-cdk-lib/aws-lambda';
import { existsSync, readFileSync } from 'node:fs';
import { resolve } from 'node:path';
import { CollectorStack } from './stack';

const app = new cdk.App();
const collectorAsset = resolve(__dirname, '../../../target/lambda/bootstrap');
const processorAsset = resolve(__dirname, '../../../target/lambda/article-processor');
const synthOnly = app.node.tryGetContext('synthOnly') === 'true';
if (!synthOnly && (!existsSync(resolve(collectorAsset, 'bootstrap')) || !existsSync(resolve(processorAsset, 'bootstrap')))) {
  throw new Error('先に mise run build:lambda を実行してください。検証専用synthは -c synthOnly=true を指定します。');
}
if (!synthOnly) {
  for (const asset of [collectorAsset, processorAsset]) {
    const binary = readFileSync(resolve(asset, 'bootstrap'));
    if (binary.length < 20 || binary.subarray(0, 4).toString('hex') !== '7f454c46' || binary.readUInt16LE(18) !== 183) {
      throw new Error('bootstrapはLinux ARM64用のELFバイナリである必要があります。');
    }
  }
}
new CollectorStack(app, 'Curatedz', {
  synthesizer: new cdk.DefaultStackSynthesizer({ qualifier: 'curatedz' }),
  // synthOnlyは構造検証専用。デプロイ時はLinux用bootstrapの実アセットが必須。
  collectorCode: synthOnly ? lambda.Code.fromAsset(resolve(__dirname, '../../../fixtures')) : lambda.Code.fromAsset(collectorAsset),
  processorCode: synthOnly ? lambda.Code.fromAsset(resolve(__dirname, '../../../fixtures')) : lambda.Code.fromAsset(processorAsset),
  scheduleEnabled: app.node.tryGetContext('scheduleEnabled') === 'true',
});
