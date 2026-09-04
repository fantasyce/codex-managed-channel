#!/bin/sh
set -eu

exec python3 - "$@" <<'PY'
from __future__ import annotations

import argparse
import re
import subprocess
import sys
import tarfile
from pathlib import Path, PurePosixPath


SKIP_PARTS = {".git", "target", "__pycache__"}


def rules() -> list[tuple[str, re.Pattern[str]]]:
    private_key = "BEGIN OPENSSH " + "PRIVATE KEY"
    public_key = "ssh-" + "ed25519"
    github_token = "gh" + "[pousr]_"
    return [
        ("home", re.compile(r"/Users/(?!user(?:[^A-Za-z0-9._-]|$)|example(?:[^A-Za-z0-9._-]|$))[A-Za-z0-9._-]+")),
        ("ipv4", re.compile(r"(?<![0-9])(?:[0-9]{1,3}\.){3}[0-9]{1,3}(?![0-9])")),
        ("thread", re.compile(r"(?<![a-z0-9])01[a-f0-9]{20,}(?![a-z0-9])", re.I)),
        ("private_key", re.compile(private_key)),
        ("public_key", re.compile(public_key + r"[ \t]+[A-Za-z0-9+/]{40,}={0,3}")),
        ("token", re.compile(github_token + r"[A-Za-z0-9]{20,}")),
        ("token", re.compile(r"(?<![A-Za-z0-9])sk-[A-Za-z0-9_-]{20,}")),
    ]


def printable_lines(data: bytes):
    if b"\0" in data:
        return
    text = data.decode("utf-8", errors="replace")
    for number, line in enumerate(text.splitlines(), 1):
        yield number, line


class Scanner:
    def __init__(self, extra_patterns: list[str]):
        self.patterns = rules()
        self.extra_patterns = extra_patterns
        self.findings = 0

    def scan(self, label: str, data: bytes) -> None:
        for line_number, line in printable_lines(data):
            for rule, pattern in self.patterns:
                if pattern.search(line):
                    self.report(rule, label, line_number)
            for literal in self.extra_patterns:
                if literal in line:
                    self.report("external_literal", label, line_number)

    def report(self, rule: str, label: str, line_number: int) -> None:
        self.findings += 1
        print(f"privacy violation: rule={rule} file={label} line={line_number}")


def load_extra_patterns(path: Path | None) -> list[str]:
    if path is None:
        return []
    values = []
    for raw in path.read_text(encoding="utf-8").splitlines():
        value = raw.strip()
        if value and not value.startswith("#"):
            values.append(value)
    return values


def scan_worktree(scanner: Scanner, root: Path, extra_file: Path | None) -> None:
    root = root.resolve()
    excluded = extra_file.resolve() if extra_file else None
    for path in sorted(root.rglob("*")):
        if not path.is_file() or any(part in SKIP_PARTS for part in path.relative_to(root).parts):
            continue
        if excluded is not None and path.resolve() == excluded:
            continue
        scanner.scan(path.relative_to(root).as_posix(), path.read_bytes())


def git(root: Path, *args: str) -> bytes:
    return subprocess.check_output(
        ["git", "-C", str(root), *args], stderr=subprocess.DEVNULL
    )


def scan_history(scanner: Scanner, root: Path) -> None:
    try:
        objects = git(root, "rev-list", "--objects", "--all").decode().splitlines()
    except (subprocess.CalledProcessError, FileNotFoundError):
        raise SystemExit("privacy scan: --history requires a Git repository with reachable commits")
    seen: set[str] = set()
    for entry in objects:
        object_id, _, object_path = entry.partition(" ")
        if not object_path or object_id in seen:
            continue
        seen.add(object_id)
        if any(part in SKIP_PARTS for part in PurePosixPath(object_path).parts):
            continue
        try:
            if git(root, "cat-file", "-t", object_id).strip() != b"blob":
                continue
            data = git(root, "cat-file", "blob", object_id)
        except subprocess.CalledProcessError:
            raise SystemExit("privacy scan: unable to read reachable Git object")
        scanner.scan("history:" + object_path, data)


def scan_archive(scanner: Scanner, archive: Path) -> None:
    try:
        with tarfile.open(archive, mode="r:*") as bundle:
            for member in bundle.getmembers():
                path = PurePosixPath(member.name)
                if member.isdir() or path.is_absolute() or ".." in path.parts:
                    if path.is_absolute() or ".." in path.parts:
                        scanner.report("unsafe_archive_path", member.name, 0)
                    continue
                if not member.isfile() or any(part in SKIP_PARTS for part in path.parts):
                    continue
                source = bundle.extractfile(member)
                if source is not None:
                    scanner.scan("archive:" + member.name, source.read())
    except (tarfile.TarError, OSError):
        raise SystemExit("privacy scan: unable to read archive")


def main() -> int:
    parser = argparse.ArgumentParser(add_help=True)
    parser.add_argument("--root", type=Path, default=Path("."))
    parser.add_argument("--history", action="store_true")
    parser.add_argument("--archive", type=Path)
    parser.add_argument("--extra-patterns", type=Path)
    args = parser.parse_args()

    if not args.root.is_dir():
        parser.error("--root must be a directory")
    extra_file = args.extra_patterns.resolve() if args.extra_patterns else None
    scanner = Scanner(load_extra_patterns(extra_file))
    scan_worktree(scanner, args.root, extra_file)
    if args.history:
        scan_history(scanner, args.root)
    if args.archive:
        scan_archive(scanner, args.archive)
    if scanner.findings:
        print(f"privacy scan failed: findings={scanner.findings}")
        return 1
    print("privacy scan passed")
    return 0


raise SystemExit(main())
PY
