"""Regression checks for local validation orchestration (no Rust build/network)."""
import importlib.util
import contextlib
import io
from pathlib import Path
import subprocess
import sys
import unittest
from unittest.mock import patch

sys.dont_write_bytecode = True
SPEC = importlib.util.spec_from_file_location("check_local", Path(__file__).with_name("check-local.py"))
CHECK = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(CHECK)


class LocalChecks(unittest.TestCase):
    def setUp(self):
        output = io.StringIO()
        self.enterContext(contextlib.redirect_stdout(output))
        self.enterContext(contextlib.redirect_stderr(output))

    def test_dry_run_never_resolves_or_runs_toolchain(self):
        with patch.object(CHECK, "environment") as environment, patch.object(CHECK.subprocess, "run") as run:
            self.assertEqual(CHECK.run_checks(list(CHECK.STEPS), 2, True), 0)
            environment.assert_not_called()
            run.assert_not_called()

    def test_failure_stops_remaining_steps(self):
        with patch.object(CHECK, "environment", return_value=("cargo", {})), patch.object(
            CHECK.subprocess, "run", return_value=subprocess.CompletedProcess([], 7)
        ) as run:
            self.assertEqual(CHECK.run_checks(["fmt", "test"], 2), 7)
            self.assertEqual(run.call_count, 1)

    def test_full_sequence_and_cache_reuse(self):
        calls = []

        def record(command, cwd, env):
            calls.append((command, cwd, env.copy()))
            return subprocess.CompletedProcess(command, 0)

        with patch.object(CHECK, "environment", return_value=("cargo", {"CARGO_NET_OFFLINE": "true"})), patch.object(
            CHECK.subprocess, "run", side_effect=record
        ):
            self.assertEqual(CHECK.run_checks(list(CHECK.STEPS), 2), 0)
        self.assertEqual(len(calls), 5)
        for (step, arguments), (command, cwd, env) in zip(CHECK.STEPS.items(), calls):
            self.assertEqual(command, ["cargo", *arguments])
            self.assertEqual(cwd, CHECK.ROOT)
            self.assertEqual(env["CARGO_BUILD_JOBS"], "2")
            self.assertEqual(env["CARGO_NET_OFFLINE"], "true")
            self.assertEqual(Path(env["CARGO_TARGET_DIR"]).name, "build-cache" if step == "build" else "embedded-git-next")
            if step != "fmt":
                self.assertIn("--offline", command)
                self.assertIn("--locked", command)

    def test_workflow_has_only_manual_trigger(self):
        workflow = (CHECK.ROOT / ".github/workflows/ci.yml").read_text(encoding="utf-8")
        trigger = workflow.split("\non:\n", 1)[1].split("\npermissions:", 1)[0]
        self.assertEqual(trigger.strip(), "workflow_dispatch:")
        self.assertIn("python scripts/check-local.py", workflow)
        self.assertIn('toolchain: ' + CHECK.tomllib.loads((CHECK.ROOT / "rust-toolchain.toml").read_text())["toolchain"]["channel"], workflow)


if __name__ == "__main__":
    unittest.main()
