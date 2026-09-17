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
NOREPLY_EMAIL = re.compile(
    r"^(?:[A-Za-z0-9-]+|[0-9]+\+[A-Za-z0-9-]+)@users\.noreply\.github\.com$",
    re.I,
)
GITHUB_SERVICE_EMAILS = {"actions@github.com", "noreply@github.com"}
CHECKSUM_FILE = re.compile(r"(?:^|[/:])(?:[^/:]+\.sha256|sha256sums)$", re.I)
CHECKSUM_LINE = re.compile(r"^[A-Fa-f0-9]{64}[ \t]+\*?(\S.*)$")


def safe_email(value: str) -> bool:
    lowered = value.lower()
    return (
        NOREPLY_EMAIL.fullmatch(value) is not None
        or lowered in GITHUB_SERVICE_EMAILS
        or lowered.endswith("@example.invalid")
    )


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
        ("email", re.compile(r"\b[A-Za-z0-9._%+-]+@[A-Za-z0-9.-]+\.[A-Za-z]{2,}\b")),
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
            if CHECKSUM_FILE.search(label):
                checksum = CHECKSUM_LINE.fullmatch(line)
                if checksum:
                    line = checksum.group(1)
            for rule, pattern in self.patterns:
                match = pattern.search(line)
                if match and rule == "email" and safe_email(match.group(0)):
                    continue
                if match:
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


def scan_git_metadata(scanner: Scanner, root: Path) -> None:
    try:
        commits = git(root, "rev-list", "--all").decode().splitlines()
    except (subprocess.CalledProcessError, FileNotFoundError):
        raise SystemExit("privacy scan: unable to read Git commit metadata")
    email_pattern = re.compile(r"<([^<>\s]+@[^<>\s]+)>")
    for commit in commits:
        try:
            raw = git(root, "cat-file", "commit", commit)
        except subprocess.CalledProcessError:
            raise SystemExit("privacy scan: unable to read Git commit metadata")
        headers, _, message = raw.partition(b"\n\n")
        for line in headers.decode("utf-8", errors="replace").splitlines():
            if not line.startswith(("author ", "committer ")):
                continue
            match = email_pattern.search(line)
            if match and not safe_email(match.group(1)):
                scanner.report("commit_email", "history:commit:" + commit[:12], 0)
        scanner.scan("history:commit:" + commit[:12], message)

    try:
        tags = git(root, "for-each-ref", "refs/tags", "--format=%(objectname)").decode().splitlines()
    except subprocess.CalledProcessError:
        raise SystemExit("privacy scan: unable to read Git tag metadata")
    for tag in tags:
        if git(root, "cat-file", "-t", tag).strip() != b"tag":
            continue
        raw = git(root, "cat-file", "tag", tag)
        headers, _, message = raw.partition(b"\n\n")
        for line in headers.decode("utf-8", errors="replace").splitlines():
            if not line.startswith("tagger "):
                continue
            match = email_pattern.search(line)
            if match and not safe_email(match.group(1)):
                scanner.report("tag_email", "history:tag:" + tag[:12], 0)
        scanner.scan("history:tag:" + tag[:12], message)


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
        scan_git_metadata(scanner, args.root)
    if args.archive:
        scan_archive(scanner, args.archive)
    if scanner.findings:
        print(f"privacy scan failed: findings={scanner.findings}")
        return 1
    print("privacy scan passed")
    return 0


raise SystemExit(main())
PY
