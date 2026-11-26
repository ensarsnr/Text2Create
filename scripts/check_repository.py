#!/usr/bin/env python3
"""Basic repository consistency checks for Text2Create.

Checks that:
  * required project files exist and are not empty,
  * relative links in Markdown files point to existing files and headings,
  * no obviously sensitive files (keys, environment files) are tracked.

Uses only the Python standard library. Run from anywhere:

    python3 scripts/check_repository.py
"""

from __future__ import annotations

import re
import subprocess
import sys
from pathlib import Path

ROOT = Path(__file__).resolve().parent.parent

REQUIRED_FILES = [
    "README.md",
    "LICENSE",
    "NOTICE",
    "PROJECT_OVERVIEW.md",
    "CONTRIBUTING.md",
    "CODE_OF_CONDUCT.md",
    "SECURITY.md",
    "THIRD_PARTY_LICENSES.md",
    ".editorconfig",
    ".gitignore",
    ".github/PULL_REQUEST_TEMPLATE.md",
    ".github/ISSUE_TEMPLATE/bug_report.yml",
    ".github/ISSUE_TEMPLATE/feature_request.yml",
    ".github/ISSUE_TEMPLATE/config.yml",
    ".github/workflows/ci.yml",
    "docs/ARCHITECTURE.md",
    "docs/DECISIONS.md",
    "docs/DEVELOPMENT.md",
    "docs/PROJECT_IR.md",
    "docs/PROJECT_STATUS.md",
    "docs/ROADMAP.md",
]

FORBIDDEN_PATTERNS = [
    re.compile(r"(^|/)\.env(\..+)?$"),
    re.compile(r"\.(pem|key|p12|pfx)$"),
    re.compile(r"(^|/)id_(rsa|dsa|ecdsa|ed25519)$"),
]
FORBIDDEN_EXCEPTIONS = {".env.example"}

LINK_PATTERN = re.compile(r"(?<!!)\[[^\]]*\]\(([^)\s]+)(?:\s+\"[^\"]*\")?\)")
FENCE_PATTERN = re.compile(r"^(```|~~~)")
HEADING_PATTERN = re.compile(r"^#{1,6}\s+(.*?)\s*#*\s*$")


def tracked_files() -> list[str]:
    """Return files tracked by Git, or all files if Git is unavailable."""
    try:
        output = subprocess.run(
            ["git", "ls-files"],
            cwd=ROOT,
            check=True,
            capture_output=True,
            text=True,
        ).stdout
        files = [line for line in output.splitlines() if line]
        if files:
            return files
    except (OSError, subprocess.CalledProcessError):
        pass
    return [
        str(path.relative_to(ROOT))
        for path in ROOT.rglob("*")
        if path.is_file() and ".git" not in path.relative_to(ROOT).parts
    ]


def github_anchor(heading: str) -> str:
    """Approximate GitHub's heading anchor generation."""
    text = re.sub(r"`([^`]*)`", r"\1", heading)
    text = re.sub(r"\[([^\]]*)\]\([^)]*\)", r"\1", text)
    text = text.strip().lower()
    text = re.sub(r"[^\w\- ]", "", text)
    return text.replace(" ", "-")


def markdown_lines(path: Path) -> list[str]:
    """Return lines outside fenced code blocks."""
    lines: list[str] = []
    in_fence = False
    for line in path.read_text(encoding="utf-8").splitlines():
        if FENCE_PATTERN.match(line.strip()):
            in_fence = not in_fence
            continue
        if not in_fence:
            lines.append(line)
    return lines


def anchors_for(path: Path) -> set[str]:
    anchors: set[str] = set()
    counts: dict[str, int] = {}
    for line in markdown_lines(path):
        match = HEADING_PATTERN.match(line)
        if not match:
            continue
        anchor = github_anchor(match.group(1))
        count = counts.get(anchor, 0)
        anchors.add(anchor if count == 0 else f"{anchor}-{count}")
        counts[anchor] = count + 1
    return anchors


def check_required_files() -> list[str]:
    errors = []
    for name in REQUIRED_FILES:
        path = ROOT / name
        if not path.is_file():
            errors.append(f"missing required file: {name}")
        elif path.stat().st_size == 0:
            errors.append(f"required file is empty: {name}")
    return errors


def check_forbidden_files(files: list[str]) -> list[str]:
    errors = []
    for name in files:
        if Path(name).name in FORBIDDEN_EXCEPTIONS:
            continue
        if any(pattern.search(name) for pattern in FORBIDDEN_PATTERNS):
            errors.append(f"potentially sensitive file is tracked: {name}")
    return errors


def check_markdown_links(files: list[str]) -> list[str]:
    errors = []
    for name in files:
        if not name.endswith(".md"):
            continue
        source = ROOT / name
        if not source.is_file():
            continue
        for line in markdown_lines(source):
            for target in LINK_PATTERN.findall(line):
                if re.match(r"^[a-z][a-z0-9+.-]*:", target, re.IGNORECASE):
                    continue  # External link (https:, mailto:, ...).
                file_part, _, anchor = target.partition("#")
                destination = (source.parent / file_part).resolve() if file_part else source
                if not destination.exists():
                    errors.append(f"{name}: broken link target '{target}'")
                    continue
                if anchor and destination.suffix == ".md":
                    if anchor.lower() not in anchors_for(destination):
                        errors.append(f"{name}: unknown anchor '#{anchor}' in '{target}'")
    return errors


def main() -> int:
    files = tracked_files()
    errors = (
        check_required_files()
        + check_forbidden_files(files)
        + check_markdown_links(files)
    )
    if errors:
        for error in errors:
            print(f"error: {error}", file=sys.stderr)
        print(f"\n{len(errors)} problem(s) found.", file=sys.stderr)
        return 1
    print(f"Repository checks passed ({len(files)} files checked).")
    return 0


if __name__ == "__main__":
    sys.exit(main())
