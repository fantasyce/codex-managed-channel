import base64
import hashlib
import os
import re
import shlex
import subprocess
import tempfile
import unittest
from pathlib import Path


PROJECT = Path(__file__).resolve().parents[1]
REMOTE_INSTALLER = PROJECT / "scripts" / "install-remote.sh"
REMOTE_UNINSTALLER = PROJECT / "scripts" / "uninstall-remote.sh"


def executable(path: Path, text: str = "#!/bin/sh\nexit 0\n"):
    path.parent.mkdir(parents=True, exist_ok=True)
    path.write_text(text, encoding="utf-8")
    path.chmod(0o755)


def fake_public_key() -> str:
    payload = base64.b64encode(b"test public key payload").decode()
    return "ssh-" + "ed25519 " + payload + " test-key\n"


class RemoteLifecycleTests(unittest.TestCase):
    def test_installed_forced_command_selects_a_fixed_client_identity(self):
        with tempfile.TemporaryDirectory() as tmp:
            home, bundle, auth, install_root, key, environment = self.prepare(Path(tmp))
            result = self.install(bundle, key, environment)
            self.assertEqual(result.returncode, 0, result.stderr)
            recorded = Path(tmp) / "args"
            executable(install_root / "bin/codex-managed-entry", f"#!/bin/sh\nprintf '%s\\n' \"$@\" > '{recorded}'\n")
            command = re.search(r'command="([^"]+)"', auth.read_text()).group(1)
            subprocess.run(shlex.split(command), env=environment, check=True)
            self.assertEqual(recorded.read_text().splitlines(), ["--client-id", "desktop"])
            state = home / ".local/state/codex-managed-channel/example-managed"
            self.assertEqual(state.stat().st_mode & 0o777, 0o700)

    def test_one_key_cannot_be_registered_for_two_managed_identities(self):
        with tempfile.TemporaryDirectory() as tmp:
            _, bundle, auth, _, key, environment = self.prepare(Path(tmp))
            self.assertEqual(self.install(bundle, key, environment).returncode, 0)
            before = auth.read_bytes()
            result = subprocess.run(["/bin/sh", str(REMOTE_INSTALLER), "--bundle", str(bundle), "--public-key", str(key), "--alias", "second-client"], env=environment, capture_output=True)
            self.assertNotEqual(result.returncode, 0)
            self.assertEqual(auth.read_bytes(), before)
    def prepare(self, root: Path):
        home = root / "home"
        bundle = root / "bundle"
        auth = home / ".ssh" / "authorized_keys"
        install_root = home / ".local" / "libexec" / "codex-managed-channel"
        public_key = root / "managed.pub"
        executable(bundle / "bin" / "codex-managed-entry")
        executable(bundle / "bin" / "codex-managed-preflight")
        executable(bundle / "scripts" / "uninstall-remote.sh")
        entries = []
        for relative in (
            "bin/codex-managed-entry",
            "bin/codex-managed-preflight",
            "scripts/uninstall-remote.sh",
        ):
            digest = hashlib.sha256((bundle / relative).read_bytes()).hexdigest()
            entries.append(f"{digest}  {relative}\n")
        (bundle / "MANIFEST.sha256").write_text("".join(entries), encoding="utf-8")
        public_key.write_text(fake_public_key(), encoding="utf-8")
        environment = {
            **os.environ,
            "HOME": str(home),
            "CODEX_MANAGED_INSTALL_ROOT": str(install_root),
            "CODEX_MANAGED_AUTHORIZED_KEYS": str(auth),
            "CODEX_MANAGED_SKIP_PREFLIGHT": "1",
            "LC_ALL": "C",
        }
        return home, bundle, auth, install_root, public_key, environment

    def install(self, bundle: Path, public_key: Path, environment: dict[str, str]):
        return subprocess.run(
            [
                "/bin/sh",
                str(REMOTE_INSTALLER),
                "--bundle",
                str(bundle),
                "--public-key",
                str(public_key),
                "--alias",
                "example-managed",
            ],
            text=True,
            capture_output=True,
            check=False,
            env=environment,
        )

    def test_install_is_idempotent_and_preserves_existing_authorizations(self):
        with tempfile.TemporaryDirectory() as tmp:
            _, bundle, auth, install_root, public_key, environment = self.prepare(Path(tmp))
            auth.parent.mkdir(parents=True)
            original = "ssh-rsa AAAATEST existing-key\n"
            auth.write_text(original, encoding="utf-8")
            first = self.install(bundle, public_key, environment)
            self.assertEqual(first.returncode, 0, first.stderr)
            once = auth.read_bytes()
            second = self.install(bundle, public_key, environment)
            self.assertEqual(second.returncode, 0, second.stderr)
            self.assertEqual(auth.read_bytes(), once)
            self.assertTrue(auth.read_text().startswith(original))
            self.assertEqual(auth.read_text().count("codex-managed-channel:example-managed"), 1)
            self.assertTrue((install_root / "bin/codex-managed-entry").is_file())

    def test_malformed_public_key_fails_before_mutation(self):
        with tempfile.TemporaryDirectory() as tmp:
            _, bundle, auth, install_root, public_key, environment = self.prepare(Path(tmp))
            public_key.write_text("not a public key\n", encoding="utf-8")
            result = self.install(bundle, public_key, environment)
            self.assertNotEqual(result.returncode, 0)
            self.assertFalse(auth.exists())
            self.assertFalse(install_root.exists())

    def test_uninstall_removes_only_matching_authorization_and_retains_state(self):
        with tempfile.TemporaryDirectory() as tmp:
            home, bundle, auth, install_root, public_key, environment = self.prepare(Path(tmp))
            result = self.install(bundle, public_key, environment)
            self.assertEqual(result.returncode, 0, result.stderr)
            auth.write_text(auth.read_text() + "ssh-rsa BBBB another-key\n", encoding="utf-8")
            log = home / ".codex-managed" / "log" / "supervisor.jsonl"
            log.parent.mkdir(parents=True)
            log.write_text("retained\n", encoding="utf-8")

            removed = subprocess.run(
                ["/bin/sh", str(REMOTE_UNINSTALLER), "--alias", "example-managed"],
                text=True,
                capture_output=True,
                check=False,
                env=environment,
            )
            self.assertEqual(removed.returncode, 0, removed.stderr)
            self.assertNotIn("codex-managed-channel:example-managed", auth.read_text())
            self.assertIn("another-key", auth.read_text())
            self.assertTrue(log.is_file())
            self.assertFalse((install_root / "installs/example-managed").exists())

    def test_purge_requires_literal_confirmation(self):
        with tempfile.TemporaryDirectory() as tmp:
            _, _, _, _, _, environment = self.prepare(Path(tmp))
            result = subprocess.run(
                ["/bin/sh", str(REMOTE_UNINSTALLER), "--alias", "example-managed", "--purge", "yes"],
                text=True,
                capture_output=True,
                check=False,
                env=environment,
            )
            self.assertNotEqual(result.returncode, 0)


if __name__ == "__main__":
    unittest.main()
