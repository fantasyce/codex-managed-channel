import json
import subprocess
import tempfile
import unittest
from pathlib import Path


PROJECT = Path(__file__).resolve().parents[1]
SCRIPT = PROJECT / "tools/bundle-third-party-licenses.py"


class ThirdPartyLicenseBundleTests(unittest.TestCase):
    def metadata(self, root: Path, dependency: Path) -> dict:
        root_id = "path+root#codex-managed-channel@0.2.0"
        dependency_id = "registry+example#sample-dependency@1.0.0"
        return {
            "packages": [
                {
                    "id": root_id,
                    "name": "codex-managed-channel",
                    "version": "0.2.0",
                    "license": "MIT",
                    "license_file": None,
                    "manifest_path": str(root / "Cargo.toml"),
                },
                {
                    "id": dependency_id,
                    "name": "sample-dependency",
                    "version": "1.0.0",
                    "license": "MIT OR Apache-2.0",
                    "license_file": None,
                    "manifest_path": str(dependency / "Cargo.toml"),
                },
            ],
            "resolve": {
                "root": root_id,
                "nodes": [
                    {"id": root_id, "dependencies": [dependency_id]},
                    {"id": dependency_id, "dependencies": []},
                ],
            },
        }

    def run_bundle(self, metadata: dict, destination: Path) -> subprocess.CompletedProcess:
        metadata_path = destination.parent / "metadata.json"
        metadata_path.write_text(json.dumps(metadata), encoding="utf-8")
        return subprocess.run(
            ["python3", str(SCRIPT), str(metadata_path), str(destination)],
            text=True,
            capture_output=True,
        )

    def test_copies_dependency_license_and_writes_notice(self):
        with tempfile.TemporaryDirectory() as temporary:
            base = Path(temporary)
            root = base / "root"
            dependency = base / "dependency"
            root.mkdir()
            dependency.mkdir()
            (root / "Cargo.toml").write_text("[package]\nname='root'\n", encoding="utf-8")
            (dependency / "Cargo.toml").write_text(
                "[package]\nname='sample-dependency'\n", encoding="utf-8"
            )
            (dependency / "LICENSE").write_text("sample license text\n", encoding="utf-8")
            destination = base / "bundle"

            result = self.run_bundle(self.metadata(root, dependency), destination)

            self.assertEqual(result.returncode, 0, result.stderr)
            notice = (destination / "THIRD_PARTY_NOTICES.md").read_text(encoding="utf-8")
            self.assertIn("sample-dependency | 1.0.0 | MIT OR Apache-2.0", notice)
            copied = destination / "THIRD_PARTY_LICENSES/sample-dependency-1.0.0/LICENSE"
            self.assertEqual(copied.read_text(encoding="utf-8"), "sample license text\n")

    def test_rejects_dependency_without_license_text(self):
        with tempfile.TemporaryDirectory() as temporary:
            base = Path(temporary)
            root = base / "root"
            dependency = base / "dependency"
            root.mkdir()
            dependency.mkdir()
            (root / "Cargo.toml").write_text("[package]\nname='root'\n", encoding="utf-8")
            (dependency / "Cargo.toml").write_text(
                "[package]\nname='sample-dependency'\n", encoding="utf-8"
            )

            result = self.run_bundle(self.metadata(root, dependency), base / "bundle")

            self.assertNotEqual(result.returncode, 0)
            self.assertIn("missing license text: sample-dependency 1.0.0", result.stderr)


if __name__ == "__main__":
    unittest.main()
