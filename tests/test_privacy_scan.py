import os
import subprocess
import tarfile
import tempfile
import unittest
from pathlib import Path


PROJECT = Path(__file__).resolve().parents[1]
SCANNER = PROJECT / "scripts" / "privacy-scan.sh"


def scan(directory: Path, *args: str) -> subprocess.CompletedProcess[str]:
    return subprocess.run(
        [str(SCANNER), "--root", str(directory), *args],
        text=True,
        capture_output=True,
        check=False,
        env={**os.environ, "LC_ALL": "C"},
    )


class PrivacyScanTests(unittest.TestCase):
    def test_rejects_identity_and_credential_shapes_without_echoing_content(self):
        bad_values = {
            "home": "/".join(("", "Users", "alice", "private")),
            "ipv4": ".".join(("198", "51", "100", "7")),
            "thread": "01" + "a" * 30,
            "private_key": "BEGIN OPENSSH " + "PRIVATE KEY",
            "public_key": "ssh-ed25519 " + "A" * 80,
            "token": "gh" + "p_" + "A" * 40,
            "email": "maintainer" + "@personal.test",
        }
        for rule, value in bad_values.items():
            with self.subTest(rule=rule), tempfile.TemporaryDirectory() as tmp:
                root = Path(tmp)
                (root / "unsafe.txt").write_text(value + "\n", encoding="utf-8")
                result = scan(root)
                self.assertNotEqual(result.returncode, 0)
                self.assertIn(rule, result.stdout)
                self.assertNotIn(value, result.stdout + result.stderr)

    def test_accepts_only_neutral_examples(self):
        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp)
            (root / "safe.txt").write_text(
                "example-host example-managed /Users/user Ed25519\n"
                "https://github.com/example/codex-managed-channel\n"
                "https://developers.openai.com/\n",
                encoding="utf-8",
            )
            result = scan(root)
            self.assertEqual(result.returncode, 0, result.stdout + result.stderr)

    def test_external_literal_file_is_applied_without_echoing_the_literal(self):
        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp)
            forbidden = "development-" + "identifier"
            (root / "unsafe.txt").write_text(forbidden, encoding="utf-8")
            patterns = root / "patterns.txt"
            patterns.write_text(forbidden + "\n", encoding="utf-8")
            result = scan(root, "--extra-patterns", str(patterns))
            self.assertNotEqual(result.returncode, 0)
            self.assertIn("external_literal", result.stdout)
            self.assertNotIn(forbidden, result.stdout + result.stderr)

    def test_rejects_sensitive_content_reachable_only_in_git_history(self):
        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp)
            subprocess.run(["git", "init", "-q", "-b", "main"], cwd=root, check=True)
            subprocess.run(["git", "config", "user.name", "Test User"], cwd=root, check=True)
            subprocess.run(
                ["git", "config", "user.email", "test@example.invalid"], cwd=root, check=True
            )
            unsafe = root / "removed.txt"
            unsafe.write_text("/".join(("", "Users", "history-owner", "secret")), encoding="utf-8")
            subprocess.run(["git", "add", "removed.txt"], cwd=root, check=True)
            subprocess.run(["git", "commit", "-q", "-m", "unsafe"], cwd=root, check=True)
            unsafe.unlink()
            subprocess.run(["git", "add", "-u"], cwd=root, check=True)
            subprocess.run(["git", "commit", "-q", "-m", "remove"], cwd=root, check=True)

            result = scan(root, "--history")
            self.assertNotEqual(result.returncode, 0)
            self.assertIn("history:removed.txt", result.stdout)

    def test_rejects_personal_email_in_commit_metadata_without_echoing_it(self):
        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp)
            exposed_email = "maintainer" + "@personal.test"
            subprocess.run(["git", "init", "-q", "-b", "main"], cwd=root, check=True)
            subprocess.run(["git", "config", "user.name", "Test User"], cwd=root, check=True)
            subprocess.run(
                ["git", "config", "user.email", exposed_email], cwd=root, check=True
            )
            (root / "safe.txt").write_text("neutral content\n", encoding="utf-8")
            subprocess.run(["git", "add", "safe.txt"], cwd=root, check=True)
            subprocess.run(["git", "commit", "-q", "-m", "safe content"], cwd=root, check=True)

            result = scan(root, "--history")
            self.assertNotEqual(result.returncode, 0)
            self.assertIn("commit_email", result.stdout)
            self.assertNotIn(exposed_email, result.stdout + result.stderr)

    def test_rejects_personal_email_in_commit_message_without_echoing_it(self):
        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp)
            exposed_email = "reviewer" + "@personal.test"
            subprocess.run(["git", "init", "-q", "-b", "main"], cwd=root, check=True)
            subprocess.run(["git", "config", "user.name", "Test User"], cwd=root, check=True)
            subprocess.run(
                ["git", "config", "user.email", "123+test@users.noreply.github.com"],
                cwd=root,
                check=True,
            )
            (root / "safe.txt").write_text("neutral content\n", encoding="utf-8")
            subprocess.run(["git", "add", "safe.txt"], cwd=root, check=True)
            subprocess.run(
                [
                    "git",
                    "commit",
                    "-q",
                    "-m",
                    "safe content",
                    "-m",
                    f"Co-authored-by: Reviewer <{exposed_email}>",
                ],
                cwd=root,
                check=True,
            )

            result = scan(root, "--history")
            self.assertNotEqual(result.returncode, 0)
            self.assertIn("history:commit:", result.stdout)
            self.assertNotIn(exposed_email, result.stdout + result.stderr)

    def test_rejects_sensitive_content_inside_release_archive(self):
        with tempfile.TemporaryDirectory() as tmp:
            base = Path(tmp)
            root = base / "root"
            payload = base / "payload"
            root.mkdir()
            payload.mkdir()
            unsafe_value = "ssh-" + "ed25519 " + "A" * 80
            (payload / "unsafe.txt").write_text(unsafe_value, encoding="utf-8")
            archive = base / "release.tar.gz"
            with tarfile.open(archive, "w:gz") as bundle:
                bundle.add(payload / "unsafe.txt", arcname="release/unsafe.txt")

            result = scan(root, "--archive", str(archive))
            self.assertNotEqual(result.returncode, 0)
            self.assertIn("archive:release/unsafe.txt", result.stdout)
            self.assertNotIn(unsafe_value, result.stdout + result.stderr)


if __name__ == "__main__":
    unittest.main()
