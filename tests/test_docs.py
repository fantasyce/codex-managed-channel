import subprocess
import unittest
from pathlib import Path


PROJECT = Path(__file__).resolve().parents[1]


class DocumentationContractTests(unittest.TestCase):
    def test_required_public_files_exist(self):
        required = (
            "README.md",
            "README.zh-CN.md",
            "LICENSE",
            "SECURITY.md",
            "PRIVACY.md",
            "CONTRIBUTING.md",
            "CHANGELOG.md",
            "docs/architecture.md",
            "docs/installation.md",
            "docs/security-model.md",
            "docs/troubleshooting.md",
            "docs/acceptance-template.md",
        )
        for relative in required:
            with self.subTest(relative=relative):
                self.assertTrue((PROJECT / relative).is_file())

    def test_readmes_state_safety_and_project_status(self):
        combined = (PROJECT / "README.md").read_text() + (PROJECT / "README.zh-CN.md").read_text()
        for phrase in (
            "unofficial",
            "no telemetry",
            "example-host",
            "example-managed",
            "ChatGPT",
            "Computer Use",
        ):
            self.assertIn(phrase.lower(), combined.lower())

    def test_installer_help_matches_documented_flags(self):
        help_result = subprocess.run(
            ["/bin/sh", str(PROJECT / "install.sh"), "--help"],
            text=True,
            capture_output=True,
            check=True,
        )
        for flag in ("--remote", "--alias", "--version", "--repository", "--key"):
            self.assertIn(flag, help_result.stdout)
            self.assertIn(flag, (PROJECT / "docs/installation.md").read_text())


if __name__ == "__main__":
    unittest.main()
