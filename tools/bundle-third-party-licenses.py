#!/usr/bin/env python3
from __future__ import annotations

import json
import re
import shutil
import sys
from pathlib import Path


SAFE_COMPONENT = re.compile(r"[A-Za-z0-9_.+-]+")
LICENSE_PREFIXES = ("license", "copying", "notice", "unlicense")


def fail(message: str) -> None:
    raise SystemExit(message)


def safe_component(value: str, label: str) -> str:
    if not SAFE_COMPONENT.fullmatch(value):
        fail(f"unsafe {label}")
    return value


def license_files(package: dict) -> list[Path]:
    manifest_dir = Path(package["manifest_path"]).resolve().parent
    candidates: set[Path] = set()
    declared = package.get("license_file")
    if declared:
        path = Path(declared)
        candidates.add(path.resolve() if path.is_absolute() else (manifest_dir / path).resolve())
    for path in manifest_dir.iterdir():
        if path.is_file() and path.name.lower().startswith(LICENSE_PREFIXES):
            candidates.add(path.resolve())
    return sorted((path for path in candidates if path.is_file()), key=lambda path: path.name.lower())


def bundle(metadata_path: Path, output: Path) -> None:
    metadata = json.loads(metadata_path.read_text(encoding="utf-8"))
    resolve = metadata.get("resolve") or fail("cargo metadata has no resolve graph")
    root_id = resolve.get("root")
    reachable = {node["id"] for node in resolve.get("nodes", [])}
    packages = {package["id"]: package for package in metadata.get("packages", [])}
    selected = [packages[package_id] for package_id in reachable if package_id != root_id]
    selected.sort(key=lambda package: (package["name"].lower(), package["version"]))

    output.mkdir(parents=True, exist_ok=True)
    license_root = output / "THIRD_PARTY_LICENSES"
    license_root.mkdir()
    rows: list[str] = []
    for package in selected:
        name = safe_component(package["name"], "package name")
        version = safe_component(package["version"], "package version")
        expression = package.get("license") or ""
        if not expression.strip():
            fail(f"missing license expression: {name} {version}")
        sources = license_files(package)
        if not sources:
            fail(f"missing license text: {name} {version}")
        package_dir = license_root / f"{name}-{version}"
        package_dir.mkdir()
        copied: set[str] = set()
        for source in sources:
            filename = safe_component(source.name, "license filename")
            if filename in copied:
                fail(f"duplicate license filename: {name} {version}")
            copied.add(filename)
            shutil.copyfile(source, package_dir / filename)
        rows.append(f"{name} | {version} | {expression.strip()}")

    notice = [
        "# Third-party notices",
        "",
        "This binary distribution contains the following Rust dependencies.",
        "Their unmodified license and notice files are included under",
        "`THIRD_PARTY_LICENSES/`.",
        "",
        "Package | Version | License expression",
        "--- | --- | ---",
        *rows,
        "",
    ]
    (output / "THIRD_PARTY_NOTICES.md").write_text("\n".join(notice), encoding="utf-8")


def main() -> None:
    if len(sys.argv) != 3:
        fail("usage: bundle-third-party-licenses.py METADATA_JSON OUTPUT_DIR")
    bundle(Path(sys.argv[1]), Path(sys.argv[2]))


if __name__ == "__main__":
    main()
