#!/usr/bin/env python3
"""Fail CI when audit producers, the domain catalog, and PostgreSQL drift."""

from __future__ import annotations

import re
import sys
from pathlib import Path

ROOT = Path(__file__).resolve().parents[1]
DOMAIN = ROOT / "crates/copilotmail-domain/src/audit.rs"
APPLICATION = ROOT / "crates/copilotmail-application/src"
MIGRATIONS = ROOT / "crates/copilotmail-postgres/migrations"

CATALOG_PATTERN = re.compile(
    r'\(\s*"(?P<action>[a-z][a-z0-9_.]+)"\s*,\s*'
    r'"(?P<resource>[a-z][a-z0-9_.]+)"\s*,\s*'
    r"(?P<risk>Low|Medium|High|Critical)\s*,\s*"
    r"(?P<privileged>true|false)\s*\)",
    re.MULTILINE,
)
SQL_CATALOG_PATTERN = re.compile(
    r"\('(?P<action>[a-z][a-z0-9_.]+)',\s*"
    r"'(?P<resource>[a-z][a-z0-9_.]+)',\s*"
    r"'(?P<risk>low|medium|high|critical)',\s*"
    r"(?P<privileged>true|false)\)"
)
SQL_CATALOG_INSERT_PATTERN = re.compile(
    r"INSERT\s+INTO\s+audit_event_catalog\s*"
    r"\((?P<columns>[^)]*)\)",
    re.IGNORECASE | re.MULTILINE,
)
ACTION_LITERAL_PATTERN = re.compile(
    r'"(?P<action>(?:organization|identity|authorization|mail|mailbox|mail_domain|mail_engine|audit|agent|compliance)'
    r'(?:\.[a-z0-9_]+)*\.(?:bootstrapped|created|bound|registered|added|'
    r"granted|revoked|issued|provisioned|adopted|updated|deleted|versioned|"
    r"requested|decided|completed|observed|dns_observed|verified|activated|suspended|"
    r'route_updated|challenge_rotated|disabled|replayed|read|opened|queried|closed|applied|released|authorized|downloaded|accepted|rejected|cancelled|review_required|admitted|captured|claimed|lease_extended|acknowledged|nacked|dead_lettered|paused|resumed|retired))"'
)
COMMIT_PATTERN = re.compile(r"transaction\.commit\(\)\.await\?;")


def fail(messages: list[str]) -> None:
    for message in messages:
        print(f"audit-governance: {message}", file=sys.stderr)
    raise SystemExit(1)


def main() -> None:
    errors: list[str] = []
    domain_text = DOMAIN.read_text(encoding="utf-8")
    domain_matches = list(CATALOG_PATTERN.finditer(domain_text))
    domain_rows = {
        (
            match["action"],
            match["resource"],
            match["risk"].lower(),
            match["privileged"],
        )
        for match in domain_matches
    }
    if not domain_rows or len(domain_rows) != len(domain_matches):
        errors.append("domain audit catalog is empty or contains duplicate rows")

    migration_texts: list[tuple[Path, str]] = []
    for migration in sorted(MIGRATIONS.glob("*.sql")):
        raw_migration = migration.read_bytes()
        if b"\xef\xbb\xbf" in raw_migration:
            errors.append(
                f"{migration.relative_to(ROOT)} contains a UTF-8 BOM; "
                "SQLx migrations must be BOM-free"
            )
        try:
            migration_text = raw_migration.decode("utf-8")
        except UnicodeDecodeError as error:
            errors.append(
                f"{migration.relative_to(ROOT)} is not valid UTF-8: {error}"
            )
            continue
        migration_texts.append((migration, migration_text))
    for migration, migration_text in migration_texts:
        for insert in SQL_CATALOG_INSERT_PATTERN.finditer(migration_text):
            columns = [
                column.strip().lower()
                for column in insert["columns"].split(",")
            ]
            if columns != ["action", "resource_type", "risk", "privileged"]:
                line = migration_text.count("\n", 0, insert.start()) + 1
                errors.append(
                    f"{migration.relative_to(ROOT)}:{line} audit catalog "
                    f"insert uses invalid columns {columns}"
                )

    sql_text = "\n".join(text for _, text in migration_texts)
    sql_rows = {
        (
            match["action"],
            match["resource"],
            match["risk"],
            match["privileged"],
        )
        for match in SQL_CATALOG_PATTERN.finditer(sql_text)
    }
    if domain_rows != sql_rows:
        errors.append(
            "domain and PostgreSQL catalogs differ: "
            f"domain_only={sorted(domain_rows - sql_rows)}, "
            f"sql_only={sorted(sql_rows - domain_rows)}"
        )

    application_actions: set[str] = set()
    for source in sorted(APPLICATION.glob("*.rs")):
        text = source.read_text(encoding="utf-8")
        application_actions.update(
            match["action"] for match in ACTION_LITERAL_PATTERN.finditer(text)
        )
        for commit in COMMIT_PATTERN.finditer(text):
            function_start = text.rfind("pub async fn ", 0, commit.start())
            transaction_prefix = text[function_start : commit.start()]
            covered = (
                "append_audit_event" in transaction_prefix
                and "enqueue_event" in transaction_prefix
            ) or (
                "append_governance_evidence" in transaction_prefix
                and "transaction.append_audit_event" in text
                and "transaction.enqueue_event" in text
            ) or (
                "append_approval_read_evidence" in transaction_prefix
                and "transaction.append_audit_event" in text
                and "transaction.enqueue_event" in text
            )
            no_op = (
                "audit-coverage: no-op-idempotent-replay"
                in transaction_prefix[-800:]
            )
            operational_source_evidence = (
                "audit-coverage: operational-source-evidence"
                in transaction_prefix[-800:]
                and "insert_inbound_mail_event" in transaction_prefix
                and "enqueue_event" in transaction_prefix
            )
            if not covered and not no_op and not operational_source_evidence:
                line = text.count("\n", 0, commit.start()) + 1
                errors.append(
                    f"{source.relative_to(ROOT)}:{line} commits without "
                    "audit+outbox or an explicit no-op marker"
                )

    domain_actions = {row[0] for row in domain_rows}
    if application_actions != domain_actions:
        errors.append(
            "application producers and governed catalog differ: "
            f"producers_only={sorted(application_actions - domain_actions)}, "
            f"catalog_only={sorted(domain_actions - application_actions)}"
        )

    if errors:
        fail(errors)
    print(
        f"audit governance ok: {len(domain_rows)} events, "
        f"{len(application_actions)} producer actions"
    )


if __name__ == "__main__":
    main()
