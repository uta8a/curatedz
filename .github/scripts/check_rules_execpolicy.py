"""ルール例を実行せず、Codex CLI に引数として渡して検証する。"""

import ast
import json
from pathlib import Path
import shlex
import subprocess
import sys

ROOT = Path(__file__).resolve().parents[2]


def examples(path):
    tree = ast.parse(path.read_text(encoding="utf-8"), filename=str(path))
    cases = []
    for statement in tree.body:
        if not (
            isinstance(statement, ast.Expr)
            and isinstance(statement.value, ast.Call)
            and isinstance(statement.value.func, ast.Name)
            and statement.value.func.id == "prefix_rule"
            and not statement.value.args
        ):
            raise ValueError(f"{path}: prefix_rule のリテラル定義のみ対応します")
        values = {
            keyword.arg: ast.literal_eval(keyword.value)
            for keyword in statement.value.keywords
        }
        decision = values.get("decision")
        if decision not in {"allow", "prompt", "forbidden"}:
            raise ValueError(f"{path}: decision が不正です")
        for field in ("match", "not_match"):
            entries = values.get(field)
            if not isinstance(entries, list) or not entries:
                raise ValueError(f"{path}: {field} に例が必要です")
            if any(not isinstance(entry, str) or not entry.strip() for entry in entries):
                raise ValueError(f"{path}: {field} は空でない文字列の配列にしてください")
        cases.extend((command, decision) for command in values["match"])
    if not cases:
        raise ValueError(f"{path}: ルールがありません")
    return cases


def check_command(rule_paths, command, expected):
    argv = ["codex", "execpolicy", "check"]
    for path in rule_paths:
        argv.extend(["--rules", str(path)])
    argv.extend(["--", *shlex.split(command)])
    try:
        result = subprocess.run(argv, capture_output=True, text=True, check=True, timeout=30)
    except subprocess.CalledProcessError as error:
        raise ValueError(f"{command}: Codex CLI が失敗しました\n{error.stderr}") from error
    # stderr の警告は JSON と混ぜない。CLI エラーや不正JSONは成功扱いにしない。
    decision = json.loads(result.stdout)["decision"]
    if decision != expected:
        raise ValueError(f"{command}: expected={expected}, actual={decision}")


def main():
    paths = sorted((ROOT / ".codex/rules").glob("*.rules"))
    if not paths:
        raise ValueError(".rules ファイルがありません")
    cases = [(command, decision) for path in paths for command, decision in examples(path)]
    # CLI は全ルールを読み込む際に match / not_match の両方を検証する。
    # 加えて、各 match についてルールを合わせた最終判定を確認する。
    for command, expected in cases:
        check_command(paths, command, expected)
    print(f"ExecPolicy: {len(cases)} 件の判定と全ルールのインライン例を検証しました")


if __name__ == "__main__":
    try:
        main()
    except (OSError, ValueError, KeyError, SyntaxError, subprocess.SubprocessError) as error:
        print(f"ExecPolicy の検証に失敗しました: {error}", file=sys.stderr)
        sys.exit(1)
