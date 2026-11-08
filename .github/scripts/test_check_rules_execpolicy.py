"""旧チェッカーの eval とエラー握りつぶしが再発しないことを確認する。"""

from pathlib import Path
import subprocess
import unittest
from unittest.mock import patch

from check_rules_execpolicy import check_command


class CheckCommandTests(unittest.TestCase):
    @patch("check_rules_execpolicy.subprocess.run")
    def test_shell_substitution_is_only_an_argument(self, run):
        run.return_value.stdout = '{"decision":"allow"}'
        check_command([Path("example.rules")], 'echo "$(touch sentinel)"', "allow")
        self.assertEqual(run.call_args.args[0][-2:], ["echo", "$(touch sentinel)"])
        self.assertNotIn("shell", run.call_args.kwargs)

    @patch("check_rules_execpolicy.subprocess.run")
    def test_cli_failure_is_not_ignored(self, run):
        run.side_effect = subprocess.CalledProcessError(1, ["codex"])
        with self.assertRaisesRegex(ValueError, "Codex CLI が失敗しました"):
            check_command([], "git status", "allow")

    @patch("check_rules_execpolicy.subprocess.run")
    def test_invalid_json_fails(self, run):
        run.return_value.stdout = "not JSON"
        with self.assertRaises(ValueError):
            check_command([], "git status", "allow")

    @patch("check_rules_execpolicy.subprocess.run")
    def test_unexpected_decision_fails(self, run):
        run.return_value.stdout = '{"decision":"allow"}'
        with self.assertRaises(ValueError):
            check_command([], "sudo reboot", "forbidden")


if __name__ == "__main__":
    unittest.main()
