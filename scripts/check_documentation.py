#!/usr/bin/env python3
"""Fail-closed governance checks for root documentation and language entrypoints."""

from __future__ import annotations

import re
import sys
from pathlib import Path
from urllib.parse import unquote

ROOT = Path(__file__).resolve().parents[1]

REQUIRED_FILES = (
    "README.md",
    "README.zh-CN.md",
    "README.en.md",
    "GOAL.md",
    "CONTRIBUTING.md",
    "SECURITY.md",
    "CHANGELOG.md",
    "LICENSE",
    "docs/IMPLEMENTATION_PLAN.zh-CN.md",
    "docs/IMPLEMENTATION_STATUS.zh-CN.md",
    "docs/THREAT_MODEL.zh-CN.md",
    "docs/openapi-v2.yaml",
    "docs/asyncapi-v2.yaml",
)

LOCAL_LINK_PATTERN = re.compile(r"!?\[[^\]]*]\(([^)\r\n]+)\)")
REQUIRED_LOCALIZED_REFERENCES = (
    "GOAL.md",
    "docs/IMPLEMENTATION_PLAN.zh-CN.md",
    "docs/IMPLEMENTATION_STATUS.zh-CN.md",
    "docs/openapi-v2.yaml",
    "docs/asyncapi-v2.yaml",
)


def read_text(relative_path: str, errors: list[str]) -> str:
    path = ROOT / relative_path
    if not path.is_file():
        errors.append(f"missing required documentation file: {relative_path}")
        return ""
    try:
        content = path.read_text(encoding="utf-8")
    except (OSError, UnicodeError) as error:
        errors.append(f"cannot read {relative_path} as UTF-8: {error}")
        return ""
    if content.startswith("\ufeff"):
        errors.append(f"{relative_path} must not contain a UTF-8 BOM")
    if len(content.strip()) < 40:
        errors.append(f"{relative_path} is empty or only a placeholder")
    if content.count("```") % 2:
        errors.append(f"{relative_path} has an unbalanced fenced code block")
    return content


def local_link_target(raw_target: str) -> str | None:
    target = raw_target.strip()
    if target.startswith("<") and target.endswith(">"):
        target = target[1:-1].strip()
    if not target or target.startswith("#"):
        return None
    lowered = target.lower()
    if lowered.startswith(("https://", "http://", "mailto:")):
        return None
    target = unquote(target.split("#", 1)[0].split("?", 1)[0])
    return target or None


def check_local_links(relative_path: str, content: str, errors: list[str]) -> None:
    source = (ROOT / relative_path).resolve()
    for match in LOCAL_LINK_PATTERN.finditer(content):
        target = local_link_target(match.group(1))
        if target is None:
            continue
        if Path(target).is_absolute():
            errors.append(f"{relative_path} uses non-portable absolute link: {target}")
            continue
        resolved = (source.parent / target).resolve()
        try:
            resolved.relative_to(ROOT.resolve())
        except ValueError:
            errors.append(f"{relative_path} link escapes repository root: {target}")
            continue
        if not resolved.exists():
            errors.append(f"{relative_path} has broken local link: {target}")


def check_language_entry(contents: dict[str, str], errors: list[str]) -> None:
    entry = contents["README.md"]
    if len(entry.encode("utf-8")) > 5_000:
        errors.append("README.md must remain a concise language and project entrypoint")
    for link in ("README.zh-CN.md", "README.en.md"):
        if f"]({link})" not in entry:
            errors.append(f"README.md does not link to {link}")

    chinese = contents["README.zh-CN.md"]
    english = contents["README.en.md"]
    for label, content in (("README.zh-CN.md", chinese), ("README.en.md", english)):
        for link in ("README.md", "README.zh-CN.md", "README.en.md"):
            if f"]({link})" not in content:
                errors.append(f"{label} language navigation does not link to {link}")
        for reference in REQUIRED_LOCALIZED_REFERENCES:
            if reference not in content:
                errors.append(f"{label} does not reference {reference}")

    if "[简体中文](README.md)" in chinese or "[简体中文](README.md)" in english:
        errors.append("the Simplified Chinese language link must target README.zh-CN.md")


def documentation_paths() -> tuple[str, ...]:
    paths = set(ROOT.glob("*.md"))
    for directory in ("docs", "deploy", "skills"):
        paths.update((ROOT / directory).rglob("*.md"))
    return tuple(sorted(path.relative_to(ROOT).as_posix() for path in paths))


def main() -> int:
    errors: list[str] = []
    contents = {path: read_text(path, errors) for path in REQUIRED_FILES}
    linked_documents = documentation_paths()
    for relative_path in linked_documents:
        contents.setdefault(relative_path, read_text(relative_path, errors))
    check_language_entry(contents, errors)

    for relative_path in linked_documents:
        check_local_links(relative_path, contents[relative_path], errors)

    if "## [Unreleased]" not in contents["CHANGELOG.md"]:
        errors.append("CHANGELOG.md must contain an Unreleased section")
    if (
        "Apache License" not in contents["LICENSE"]
        or "Version 2.0, January 2004" not in contents["LICENSE"]
    ):
        errors.append("LICENSE must contain the complete Apache-2.0 license")

    if errors:
        for error in errors:
            print(f"documentation governance error: {error}", file=sys.stderr)
        return 1

    print(
        "documentation governance ok: "
        f"{len(REQUIRED_FILES)} required files, "
        f"{len(linked_documents)} link-checked documents, "
        "3 synchronized language entrypoints"
    )
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
