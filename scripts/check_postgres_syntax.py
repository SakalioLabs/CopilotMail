#!/usr/bin/env python3
"""Parse migrations without opening a connection or executing any SQL.

This catches grammar errors, including PL/pgSQL bodies, before deployment.
It cannot resolve schema references, prove trigger behavior, or replace the
isolated PostgreSQL integration suite.
"""
from __future__ import annotations

import argparse
from pathlib import Path
import sys
import unittest

from pglast import parser

ROOT = Path(__file__).resolve().parents[1]


def validate(source: str) -> None:
    if "\ufeff" in source:
        raise ValueError("SQLx migrations must not contain a UTF-8 BOM")
    parser.parse_sql(source)
    parser.parse_plpgsql_json(source)


class SyntaxTests(unittest.TestCase):
    def test_valid_sql_and_trigger_body(self):
        validate("CREATE TABLE fixture (id uuid PRIMARY KEY);")
        validate("CREATE FUNCTION f() RETURNS trigger LANGUAGE plpgsql AS $$ BEGIN RETURN NEW; END; $$;")

    def test_broken_dollar_quote_is_rejected(self):
        with self.assertRaises(parser.ParseError):
            validate("CREATE FUNCTION f() RETURNS trigger LANGUAGE plpgsql AS $ BEGIN RETURN NEW; END; $$;")

    def test_invalid_plpgsql_body_is_not_just_an_accepted_sql_string(self):
        source = "CREATE FUNCTION f() RETURNS trigger LANGUAGE plpgsql AS $$ BEGIN IF THEN RETURN NEW; END; $$;"
        parser.parse_sql(source)  # Outer SQL grammar alone cannot find this.
        with self.assertRaises(parser.ParseError):
            validate(source)

    def test_bom_is_rejected(self):
        with self.assertRaises(ValueError):
            validate("\ufeffSELECT 1;")


def main() -> int:
    args = argparse.ArgumentParser(description=__doc__)
    args.add_argument("--self-test", action="store_true")
    options = args.parse_args()
    if parser.get_postgresql_version()[0] != 17:
        print("PostgreSQL syntax gate requires the pinned PG17 grammar", file=sys.stderr)
        return 1
    if options.self_test:
        result = unittest.TextTestRunner().run(unittest.defaultTestLoader.loadTestsFromTestCase(SyntaxTests))
        if not result.wasSuccessful():
            return 1
    paths = sorted((ROOT / "crates/copilotmail-postgres/migrations").glob("*.sql"))
    migrations = len(paths)
    paths += sorted((ROOT / "crates/copilotmail-postgres/src/schema").glob("*.sql"))
    errors = []
    for path in paths:
        try:
            validate(path.read_text(encoding="utf-8"))
        except (ValueError, parser.ParseError) as error:
            errors.append(f"{path.relative_to(ROOT).as_posix()}: {error}")
    if errors:
        print("\n".join(errors), file=sys.stderr)
        return 1
    print(f"PostgreSQL 17 syntax: {migrations} migrations / {len(paths) - migrations} schema queries parsed; no SQL executed")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
