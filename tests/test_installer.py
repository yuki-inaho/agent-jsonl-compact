"""Exercise install.sh offline with real tar/checksum tools and synthetic releases."""
import hashlib
import io
import json
import os
import shutil
from pathlib import Path
import subprocess
import tarfile
import tempfile
import unittest

ROOT = Path(__file__).resolve().parents[1]
BIN = "agent-jsonl-compact"


class InstallerTests(unittest.TestCase):
    def setUp(self):
        self.temporary = tempfile.TemporaryDirectory(prefix="ajc-installer-")
        self.addCleanup(self.temporary.cleanup)
        self.root = Path(self.temporary.name)
        self.mock = self.root / "mock"
        self.mock.mkdir()
        self.destination = self.root / "bin with spaces"
        self.destination.mkdir()
        self.installed = self.destination / BIN
        self.installed.write_text("previous installation")
        self.env = dict(os.environ, PATH=f"{self.mock}:{os.environ['PATH']}",
                        INSTALL_DIR=str(self.destination), AJC_VERSION="v1.2.3",
                        AJC_TEST_FIXTURES=str(self.root), AJC_TEST_OS="Linux",
                        AJC_TEST_ARCH="x86_64")
        self.script("uname", """#!/usr/bin/env python3
import os, sys
print(os.environ['AJC_TEST_OS'] if sys.argv[1] == '-s' else os.environ['AJC_TEST_ARCH'])
""")
        self.script("curl", """#!/usr/bin/env python3
import os, pathlib, shutil, sys
a = sys.argv[1:]
assert a[a.index('--proto') + 1] == '=https'
assert a[a.index('--proto-redir') + 1] == '=https'
assert '--tlsv1.2' in a and '--max-time' in a
root = pathlib.Path(os.environ['AJC_TEST_FIXTURES'])
if os.environ.get('AJC_TEST_DOWNLOAD_FAIL'):
    sys.exit(22)
if '-o' in a:
    url = next(v for v in a if v.startswith('https://'))
    shutil.copyfile(root / url.rsplit('/', 1)[1], a[a.index('-o') + 1])
else:
    print((root / 'release.json').read_text())
""")
        self.archive()

    def script(self, name, content):
        path = self.mock / name
        path.write_text(content)
        path.chmod(0o755)

    def archive(self, target="x86_64-unknown-linux-musl", attack=None):
        directory = f"{BIN}-{target}"
        self.asset = f"{directory}.tar.gz"
        archive = self.root / self.asset
        with tarfile.open(archive, "w:gz") as tar:
            def add(name, body=b"fixture"):
                info = tarfile.TarInfo(name)
                info.size = len(body)
                info.mode = 0o755
                tar.addfile(info, io.BytesIO(body))
            # Installation must never execute this synthetic downloaded program.
            add(f"{directory}/{BIN}", f"#!/bin/sh\ntouch '{self.root}/executed'\n".encode())
            add(f"{directory}/README.md")
            add(f"{directory}/README.en.md")
            add(f"{directory}/SECURITY.md")
            add(f"{directory}/SECURITY.en.md")
            add(f"{directory}/CHANGELOG.md")
            add(f"{directory}/CHANGELOG.ja.md")
            add(f"{directory}/docs/CLI.md")
            add(f"{directory}/docs/adr/0007-untrusted-log-evidence.md")
            add(f"{directory}/LICENSE")
            if attack == "traversal":
                add("../escaped")
            elif attack == "absolute":
                add(str(self.root / "escaped"))
            elif attack == "duplicate":
                add(f"{directory}/{BIN}")
            elif attack in ("symlink", "hardlink"):
                # Replace the earlier binary with a single linked member.
                pass
        if attack in ("symlink", "hardlink"):
            with tarfile.open(archive, "w:gz") as tar:
                info = tarfile.TarInfo(f"{directory}/{BIN}")
                info.type = tarfile.SYMTYPE if attack == "symlink" else tarfile.LNKTYPE
                info.linkname = str(self.installed)
                tar.addfile(info)
        digest = hashlib.sha256(archive.read_bytes()).hexdigest()
        self.checksum = self.root / f"{self.asset}.sha256"
        self.checksum.write_text(f"{digest}  {self.asset}\n")
        (self.root / "release.json").write_text(json.dumps({"tag_name": "v1.2.3"}, indent=2))

    def run_installer(self, success):
        result = subprocess.run(["bash", str(ROOT / "install.sh")], env=self.env,
                                text=True, capture_output=True, timeout=20)
        self.assertEqual(result.returncode == 0, success, result.stdout + result.stderr)
        self.assertFalse((self.root / "executed").exists(), "downloaded program was executed")
        self.assertFalse((self.root / "escaped").exists())
        self.assertEqual(list(self.destination.glob(f".{BIN}.*")), [], "staging file leaked")
        if not success:
            self.assertEqual(self.installed.read_text(), "previous installation")
        return result

    def test_install_supported_platforms(self):
        for operating_system, architecture, target in [
            ("Linux", "x86_64", "x86_64-unknown-linux-musl"),
            ("Darwin", "arm64", "aarch64-apple-darwin"),
            ("Darwin", "x86_64", "x86_64-apple-darwin"),
        ]:
            with self.subTest(target=target):
                self.env.update(AJC_TEST_OS=operating_system, AJC_TEST_ARCH=architecture)
                self.archive(target)
                self.run_installer(True)
                self.assertEqual(self.installed.stat().st_mode & 0o777, 0o755)

    def test_latest_release(self):
        self.env["AJC_VERSION"] = "latest"
        self.run_installer(True)

    def test_shasum_fallback(self):
        if not shutil.which("shasum"):
            self.skipTest("shasum is not installed")
        # Exercise the macOS checksum path even on a GNU/Linux host.
        for command in ["bash", "python3", "tar", "gzip", "mktemp", "install", "mkdir",
                        "mv", "awk", "sed", "cat", "rm", "shasum"]:
            (self.mock / command).symlink_to(shutil.which(command))
        self.env["PATH"] = str(self.mock)
        self.run_installer(True)

    def test_invalid_tags(self):
        for tag in ["../main", 'v1.2.3";touch injected', "v1.2.3\nextra", "main"]:
            with self.subTest(tag=tag):
                self.env["AJC_VERSION"] = tag
                self.run_installer(False)

    def test_invalid_latest_metadata(self):
        self.env["AJC_VERSION"] = "latest"
        (self.root / "release.json").write_text('{"tag_name": "../bad"}')
        self.run_installer(False)

    def test_bad_checksums(self):
        for body in ["", "a" * 64 + "  wrong.tar.gz\n", "x  archive\n",
                     "0" * 64 + f"  {self.asset}\n", self.checksum.read_text() * 2]:
            with self.subTest(checksum=body):
                self.checksum.write_text(body)
                self.run_installer(False)

    def test_malicious_archives(self):
        for attack in ["traversal", "absolute", "duplicate", "symlink", "hardlink"]:
            with self.subTest(attack=attack):
                self.archive(attack=attack)
                self.run_installer(False)

    def test_destination_symlink(self):
        victim = self.root / "victim"
        self.installed.rename(victim)
        self.installed.symlink_to(victim)
        self.run_installer(False)
        self.assertTrue(self.installed.is_symlink())

    def test_destination_hardlink_preserves_other_name(self):
        victim = self.root / "victim"
        os.link(self.installed, victim)
        self.run_installer(True)
        self.assertEqual(victim.read_text(), "previous installation")

    def test_relative_destination_is_rejected(self):
        self.env["INSTALL_DIR"] = "relative-bin"
        self.run_installer(False)

    def test_download_failure(self):
        self.env["AJC_TEST_DOWNLOAD_FAIL"] = "1"
        self.run_installer(False)

    def test_unsupported_platform(self):
        self.env["AJC_TEST_ARCH"] = "aarch64"
        self.run_installer(False)


if __name__ == "__main__":
    unittest.main()
