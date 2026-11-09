"""ログイン済みCLIで専用bootstrapとGitHub変数を準備する。認証情報は扱わない。"""

import argparse
import json
import os
from pathlib import Path
import re
import subprocess

ROOT = Path(__file__).resolve().parents[1]


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--repository", required=True)
    args = parser.parse_args()
    if not re.fullmatch(r"[A-Za-z0-9_.-]+/[A-Za-z0-9_.-]+", args.repository):
        raise SystemExit("repositoryはOWNER/REPOで指定してください")
    expected = os.environ.get("EXPECTED_AWS_ACCOUNT_ID", "")
    if not re.fullmatch(r"\d{12}", expected):
        raise SystemExit("EXPECTED_AWS_ACCOUNT_IDに確認済みの12桁IDを指定してください")
    profile = os.environ.get("AWS_PROFILE", "curatedz")
    region = os.environ.get("AWS_REGION", "ap-northeast-1")
    aws = ["aws", "--profile", profile, "--region", region]

    def query(*command):
        result = subprocess.run([*aws, *command, "--output", "json"], check=True, capture_output=True, text=True)
        return json.loads(result.stdout)

    account = query("sts", "get-caller-identity")["Account"]
    if account != expected:
        raise SystemExit("ログイン先とEXPECTED_AWS_ACCOUNT_IDが一致しません")
    try:
        stack = query("cloudformation", "describe-stacks", "--stack-name", "CuratedzDeploymentAccess")["Stacks"][0]
        parameters = {item["ParameterKey"]: item["ParameterValue"] for item in stack["Parameters"]}
        # 自分のスタックが管理するProviderを再実行時に削除しない。
        create_provider = parameters["CreateOidcProvider"]
        if parameters["GitHubRepository"] != args.repository:
            raise SystemExit("既存OIDCロールの対象リポジトリが異なります。手動確認が必要です")
    except subprocess.CalledProcessError as error:
        if "does not exist" not in error.stderr:
            raise
        providers = query("iam", "list-open-id-connect-providers")["OpenIDConnectProviderList"]
        create_provider = str(not any(item["Arn"].endswith("/token.actions.githubusercontent.com") for item in providers)).lower()

    subprocess.run([*aws, "cloudformation", "deploy", "--stack-name", "CuratedzDeploymentAccess",
                    "--template-file", str(ROOT / "infra/bootstrap/access.template.json"),
                    "--capabilities", "CAPABILITY_NAMED_IAM", "--parameter-overrides",
                    f"GitHubRepository={args.repository}", f"CreateOidcProvider={create_provider}",
                    "--no-fail-on-empty-changeset"], check=True)
    subprocess.run([*aws, "cloudformation", "update-termination-protection", "--enable-termination-protection",
                    "--stack-name", "CuratedzDeploymentAccess"], check=True, stdout=subprocess.DEVNULL)
    outputs = query("cloudformation", "describe-stacks", "--stack-name", "CuratedzDeploymentAccess")["Stacks"][0]["Outputs"]
    outputs = {item["OutputKey"]: item["OutputValue"] for item in outputs}
    subprocess.run(["npm", "run", "cdk", "--", "bootstrap", f"aws://{account}/{region}",
                    "--profile", profile, "--toolkit-stack-name", "CuratedzToolkit", "--qualifier", "curatedz",
                    "--template", "bootstrap/toolkit.template.json", "--cloudformation-execution-policies",
                    outputs["ExecutionPolicyArn"], "--termination-protection"], cwd=ROOT / "infra", check=True)
    for name, value in {"AWS_ACCOUNT_ID": account, "AWS_REGION": region, "AWS_DEPLOY_ROLE_ARN": outputs["GitHubRoleArn"]}.items():
        subprocess.run(["gh", "variable", "set", name, "--repo", args.repository, "--body", value], check=True)
    print("専用bootstrapとGitHub Actions変数の準備が完了しました。")


if __name__ == "__main__":
    main()
