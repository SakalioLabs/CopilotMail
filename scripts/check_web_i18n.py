#!/usr/bin/env python3
"""Fail closed when the Human mail UI bypasses its locale catalog."""

from __future__ import annotations

import argparse
import pathlib
import re
import subprocess
import sys

ROOT = pathlib.Path(__file__).resolve().parents[1]
MAIL_APP_SOURCE = ROOT / "web" / "mail-app" / "src"
CATALOG = MAIL_APP_SOURCE / "i18n.ts"
AST_CHECKER = ROOT / "scripts" / "check_web_i18n_ast.mjs"
HAN_PATTERN = re.compile(r"[\u3400-\u4dbf\u4e00-\u9fff\uf900-\ufaff]")
TEST_DIRECTORY_NAMES = {"test", "tests", "__tests__"}


class I18nError(ValueError):
    """Production UI text escaped the governed locale catalog."""


def production_sources(directory: pathlib.Path) -> list[pathlib.Path]:
    return sorted(
        path
        for path in directory.rglob("*")
        if path.is_file()
        and path.suffix in {".ts", ".tsx"}
        and path.name != "i18n.ts"
        and ".test." not in path.name
        and ".spec." not in path.name
        and not path.name.endswith(".d.ts")
        and not TEST_DIRECTORY_NAMES.intersection(
            part.lower() for part in path.relative_to(directory).parts[:-1]
        )
    )


def han_locations(text: str) -> list[int]:
    return [
        line_number
        for line_number, line in enumerate(text.splitlines(), start=1)
        if HAN_PATTERN.search(line)
    ]


def validate() -> None:
    if not CATALOG.is_file():
        raise I18nError("mail-app locale catalog is missing")
    catalog = CATALOG.read_text(encoding="utf-8")
    for marker in ('"zh-CN"', "const en =", "export type MessageKey"):
        if marker not in catalog:
            raise I18nError(f"mail-app locale catalog is missing {marker}")

    violations: list[str] = []
    for source in production_sources(MAIL_APP_SOURCE):
        for line_number in han_locations(source.read_text(encoding="utf-8")):
            violations.append(
                f"{source.relative_to(ROOT).as_posix()}:{line_number}"
            )
    if violations:
        raise I18nError(
            "Chinese production UI literals must live in i18n.ts: "
            + ", ".join(violations)
        )


def validate_jsx_ast(run_self_test: bool) -> None:
    if not AST_CHECKER.is_file():
        raise I18nError("mail-app JSX AST checker is missing")
    command = ["node", str(AST_CHECKER)]
    if run_self_test:
        command.append("--self-test")
    try:
        result = subprocess.run(
            command,
            cwd=ROOT,
            capture_output=True,
            check=False,
            encoding="utf-8",
            text=True,
            timeout=30,
        )
    except subprocess.TimeoutExpired as error:
        raise I18nError("mail-app JSX AST checker timed out") from error
    except OSError as error:
        raise I18nError(
            "unable to run the JSX AST checker; run npm ci in web first"
        ) from error
    if result.returncode != 0:
        detail = result.stderr.strip() or result.stdout.strip() or "unknown AST failure"
        raise I18nError(detail)


def self_test() -> None:
    if han_locations('const label = "\u6536\u4ef6\u7bb1";') != [1]:
        raise I18nError("self-test failed to detect a Chinese UI literal")
    if han_locations('const key = "nav.inbox";'):
        raise I18nError("self-test rejected a locale key")


def main() -> None:
    parser = argparse.ArgumentParser(
        description="Check Human mail UI locale-catalog ownership"
    )
    parser.add_argument(
        "--self-test",
        action="store_true",
        help="exercise the detector before checking repository sources",
    )
    args = parser.parse_args()
    try:
        if args.self_test:
            self_test()
        validate()
        validate_jsx_ast(args.self_test)
    except (OSError, UnicodeError, I18nError) as error:
        print(f"web i18n gate failed: {error}", file=sys.stderr)
        raise SystemExit(1) from error
    print(
        "web i18n gate passed: Human mail production text and visible JSX "
        "literals are catalog-owned for zh-CN and en"
    )


if __name__ == "__main__":
    main()
