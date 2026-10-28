"""Regression tests for justfile argument handling (F-02)."""

from __future__ import annotations

import os
import shutil
import stat
import subprocess
import tempfile
import unittest
from pathlib import Path

ROOT = Path(__file__).resolve().parents[1]


@unittest.skipIf(shutil.which("just") is None, "just is not installed")
class JustfileArgumentTests(unittest.TestCase):
    def test_stats_passes_shell_metacharacters_as_one_argument(self) -> None:
        with tempfile.TemporaryDirectory() as tmp:
            temp = Path(tmp)
            fake_bin = temp / "bin"
            fake_bin.mkdir()
            cargo = fake_bin / "cargo"
            cargo.write_text(
                "#!/usr/bin/env bash\n"
                "printf '%s\\0' \"$@\" > \"$AJC_ARGV_FILE\"\n",
                encoding="utf-8",
            )
            cargo.chmod(cargo.stat().st_mode | stat.S_IXUSR)

            marker = temp / "MARKER"
            weird = f"$(touch {marker}) 'quote' \"double\" line\nbreak.jsonl"
            argv_file = temp / "argv.txt"
            env = os.environ.copy()
            env["PATH"] = f"{fake_bin}:{env['PATH']}"
            env["AJC_ARGV_FILE"] = str(argv_file)

            completed = subprocess.run(
                [shutil.which("just"), "stats", weird],
                cwd=ROOT,
                env=env,
                text=True,
                capture_output=True,
                check=False,
            )
            self.assertEqual(completed.returncode, 0, completed.stderr)
            self.assertFalse(marker.exists(), "command substitution was evaluated")
            args = argv_file.read_bytes().split(b"\0")[:-1]
            self.assertEqual(
                args,
                [
                    b"run",
                    b"--release",
                    b"--",
                    b"-i",
                    weird.encode(),
                    b"--stats",
                ],
            )


if __name__ == "__main__":
    unittest.main()
