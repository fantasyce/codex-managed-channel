import unittest
from pathlib import Path


PROJECT = Path(__file__).resolve().parents[1]


class PackageContractTests(unittest.TestCase):
    def test_package_script_includes_every_remote_lifecycle_file(self):
        script = (PROJECT / "scripts/package-release.sh").read_text(encoding="utf-8")
        for relative in (
            "codex-managed-entry",
            "codex-managed-preflight",
            "install-remote.sh",
            "uninstall-remote.sh",
            "MANIFEST.sha256",
        ):
            self.assertIn(relative, script)

if __name__ == "__main__":
    unittest.main()
