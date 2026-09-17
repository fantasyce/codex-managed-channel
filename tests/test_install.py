import hashlib
import io
import os
import subprocess
import tarfile
import tempfile
import unittest
from pathlib import Path


PROJECT = Path(__file__).resolve().parents[1]
INSTALLER = PROJECT / "install.sh"
LIBRARY = PROJECT / "scripts" / "install-lib.sh"


def run_installer(home: Path, *args: str) -> subprocess.CompletedProcess[str]:
    return subprocess.run(
        ["/bin/sh", str(INSTALLER), *args],
        text=True,
        capture_output=True,
        check=False,
        env={**os.environ, "HOME": str(home), "LC_ALL": "C"},
    )


def run_library(command: str, *args: str) -> subprocess.CompletedProcess[str]:
    return subprocess.run(
        ["/bin/sh", "-c", f'. "$1"; shift; {command} "$@"', "test", str(LIBRARY), *args],
        text=True,
        capture_output=True,
        check=False,
        env={**os.environ, "LC_ALL": "C"},
    )


class InstallerTests(unittest.TestCase):
    def test_alias_cannot_name_the_state_directory_or_its_parent(self):
        for alias in (".", ".."):
            self.assertNotEqual(run_library("validate_alias", alias).returncode, 0)
    def test_rendered_config_detects_dead_transport_without_sharing_master(self):
        with tempfile.TemporaryDirectory() as tmp:
            config = Path(tmp) / "config"
            result = run_library("write_managed_config", str(config), "example-managed", "example.invalid", "user", "22", "/tmp/example-key", "none")
            self.assertEqual(result.returncode, 0, result.stderr)
            resolved = subprocess.run(["ssh", "-G", "-F", str(config), "example-managed"], capture_output=True, text=True)
            self.assertEqual(resolved.returncode, 0)
            options = dict(line.split(" ", 1) for line in resolved.stdout.splitlines())
            self.assertEqual(options["serveraliveinterval"], "15")
            self.assertEqual(options["serveralivecountmax"], "3")
            self.assertEqual(options["controlmaster"], "false")
    def assert_no_mutation(self, home: Path):
        self.assertFalse((home / ".ssh" / "codex-managed-channel").exists())
        self.assertFalse((home / ".ssh" / "config").exists())

    def test_help_documents_required_arguments(self):
        result = run_installer(Path(tempfile.gettempdir()), "--help")
        self.assertEqual(result.returncode, 0, result.stderr)
        for flag in ("--remote", "--alias", "--version", "--repository", "--key"):
            self.assertIn(flag, result.stdout)

    def test_invalid_or_equal_aliases_fail_before_mutation(self):
        for arguments in (
            ("--remote", "example host", "--alias", "example-managed"),
            ("--remote", "same-host", "--alias", "same-host"),
            ("--remote", "example-host", "--alias", "../managed"),
            ("--remote", "example-host", "--alias", "example-managed", "--version", "v0.1.0';touch unsafe"),
            ("--remote", "example-host", "--alias", "example-managed", "--repository", "owner/repo;unsafe"),
        ):
            with self.subTest(arguments=arguments), tempfile.TemporaryDirectory() as tmp:
                home = Path(tmp)
                result = run_installer(home, *arguments)
                self.assertNotEqual(result.returncode, 0)
                self.assert_no_mutation(home)

    def test_existing_unmanaged_alias_is_rejected_without_mutation(self):
        with tempfile.TemporaryDirectory() as tmp:
            home = Path(tmp)
            ssh = home / ".ssh"
            ssh.mkdir()
            config = ssh / "config"
            original = "Host example-managed\n  HostName example.invalid\n"
            config.write_text(original, encoding="utf-8")
            result = run_installer(
                home, "--remote", "example-host", "--alias", "example-managed"
            )
            self.assertNotEqual(result.returncode, 0)
            self.assertEqual(config.read_text(encoding="utf-8"), original)
            self.assertFalse((ssh / "codex-managed-channel").exists())

    def test_checksum_verification_accepts_exact_entry_and_rejects_mismatch(self):
        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp)
            artifact = root / "release.tar.gz"
            artifact.write_bytes(b"release payload")
            digest = hashlib.sha256(artifact.read_bytes()).hexdigest()
            sums = root / "SHA256SUMS"
            sums.write_text(f"{digest}  {artifact.name}\n", encoding="utf-8")
            good = run_library("verify_checksum", str(artifact), str(sums))
            self.assertEqual(good.returncode, 0, good.stderr)

            sums.write_text(f"{'0' * 64}  {artifact.name}\n", encoding="utf-8")
            bad = run_library("verify_checksum", str(artifact), str(sums))
            self.assertNotEqual(bad.returncode, 0)

    def test_config_renderer_is_idempotent_and_preserves_other_hosts(self):
        with tempfile.TemporaryDirectory() as tmp:
            config = Path(tmp) / "config"
            config.write_text("Host other-host\n  HostName other.invalid\n", encoding="utf-8")
            args = (
                str(config),
                "example-managed",
                "example.invalid",
                "user",
                "22",
                "/Users/user/.ssh/codex-managed-channel/example-managed",
                "none",
            )
            first = run_library("write_managed_config", *args)
            self.assertEqual(first.returncode, 0, first.stderr)
            once = config.read_bytes()
            second = run_library("write_managed_config", *args)
            self.assertEqual(second.returncode, 0, second.stderr)
            self.assertEqual(config.read_bytes(), once)
            self.assertIn(b"Host other-host", once)
            self.assertEqual(once.count(b"BEGIN codex-managed-channel example-managed"), 1)

    def test_archive_path_validation_rejects_traversal(self):
        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp)
            safe = root / "safe.tar.gz"
            with tarfile.open(safe, "w:gz") as bundle:
                payload = b"safe"
                info = tarfile.TarInfo("codex-managed-channel/bin/example")
                info.size = len(payload)
                bundle.addfile(info, io.BytesIO(payload))
            self.assertEqual(run_library("verify_archive_paths", str(safe)).returncode, 0)

            unsafe = root / "unsafe.tar.gz"
            with tarfile.open(unsafe, "w:gz") as bundle:
                payload = b"unsafe"
                info = tarfile.TarInfo("../unsafe")
                info.size = len(payload)
                bundle.addfile(info, io.BytesIO(payload))
            self.assertNotEqual(run_library("verify_archive_paths", str(unsafe)).returncode, 0)


if __name__ == "__main__":
    unittest.main()
