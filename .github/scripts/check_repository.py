"""実装前のリポジトリ設定とローカルMarkdownリンクを検証する。"""

import json
from pathlib import Path
import re
import subprocess
import sys
import tomllib
from urllib.parse import unquote

ROOT = Path(__file__).resolve().parents[2]


def main():
    result = subprocess.run(
        ["git", "ls-files", "--cached", "--others", "--exclude-standard", "-z"],
        cwd=ROOT, capture_output=True, check=True,
    )
    paths = sorted({ROOT / name.decode() for name in result.stdout.split(b"\0") if name})
    checked = 0
    for path in paths:
        if not path.is_file():
            continue
        if path.suffix == ".json":
            json.loads(path.read_text(encoding="utf-8"))
            checked += 1
        elif path.suffix == ".toml":
            tomllib.loads(path.read_text(encoding="utf-8"))
            checked += 1
        elif path.suffix == ".md":
            # このリポジトリで使う単純なインラインリンクを対象とする。
            for target in re.findall(r"\]\(([^\s)]+)\)", path.read_text(encoding="utf-8")):
                if ":" in target or target.startswith("#"):
                    continue
                local = unquote(target.split("#", 1)[0])
                if not (path.parent / local).exists():
                    raise ValueError(f"{path.relative_to(ROOT)}: リンク先がありません: {target}")
            checked += 1
    subprocess.run(["git", "diff", "--check"], cwd=ROOT, check=True)
    print(f"設定・ドキュメント: {checked} ファイルを検証しました")


if __name__ == "__main__":
    try:
        main()
    except (OSError, ValueError, subprocess.SubprocessError) as error:
        print(f"リポジトリの検証に失敗しました: {error}", file=sys.stderr)
        sys.exit(1)
