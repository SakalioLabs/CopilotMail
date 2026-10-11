#!/usr/bin/env python3
"""Fail CI when audit producers, the domain catalog, and PostgreSQL drift."""

from __future__ import annotations

import re
import sys
from pathlib import Path
if __package__:
    from .rust_source import functions, masked, production_source
else:
    from rust_source import functions, masked, production_source

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
COMMIT_PATTERN = re.compile(r"\b(?P<receiver>[A-Za-z_]\w*)\s*\.\s*commit\s*\(\s*\)\s*\.\s*await\b")


def transaction_coverage_errors(source: Path, raw: str) -> list[str]:
    production = production_source(raw)
    code = masked(production)
    without_comments = masked(production, keep_strings=True)
    comments = "".join(char if char != without_comments[index] else ("\n" if char == "\n" else " ")
                       for index, char in enumerate(production))
    scopes = functions(code)
    helpers = {item.name: code[item.body_start:item.end] for item in scopes}
    helper_parameters = {}
    for item in scopes:
        signature = code[item.start:item.body_start]
        parameters = re.findall(r"\b([A-Za-z_]\w*)\s*:(?!:)", signature[signature.find("(") + 1:])
        helper_parameters[item.name] = parameters[0] if parameters else None

    def operations(prefix: str, receiver: str, visited: frozenset[str] = frozenset()) -> set[str]:
        found = set(re.findall(r"\b" + re.escape(receiver) + r"\s*\.\s*(append_audit_event|enqueue_event)\s*\(", prefix))
        for call in re.finditer(r"\b([a-z_]\w*)\s*\(", prefix):
            name = call[1]
            if prefix[:call.start()].rstrip().endswith((".", ":")): continue
            same_argument = re.match(r"\s*&?\s*(?:mut\s+)?\*?\s*" + re.escape(receiver) + r"\s*(?=,|\))", prefix[call.end():])
            parameter = helper_parameters.get(name)
            if name in helpers and name not in visited and same_argument and parameter:
                # A helper which allocates/shadows its own transaction cannot
                # prove evidence for its caller's different transaction.
                shadowed = re.search(r"\blet\s+(?:mut\s+)?" + re.escape(parameter) + r"\b", helpers[name])
                if not shadowed:
                    found.update(operations(helpers[name], parameter, visited | {name}))
        return found

    errors = []
    for commit in COMMIT_PATTERN.finditer(code):
        owners = [item for item in scopes if item.body_start < commit.start() < item.end]
        if not owners:
            errors.append(f"{source}:{raw.count(chr(10), 0, commit.start()) + 1} commit outside a recognized function")
            continue
        owner = max(owners, key=lambda item: item.start)
        prefix = code[owner.body_start:commit.start()]
        receiver = commit["receiver"]
        annotation = comments[max(owner.body_start, commit.start() - 800):commit.start()]
        covered = {"append_audit_event", "enqueue_event"}.issubset(operations(prefix, receiver))
        no_op = "audit-coverage: no-op-idempotent-replay" in annotation
        operational = (
            "audit-coverage: operational-source-evidence" in annotation
            and any(name in prefix for name in ("insert_inbound_mail_event", "insert_outbound_mail_event", "append_mail_delivery_snapshot"))
            and "enqueue_event" in operations(prefix, receiver)
        )
        lease = (
            "audit-coverage: operational-lease" in annotation
            and source.name == "mail_submission.rs" and owner.name == "execute"
            and "acquire_mail_submission_lease" in prefix
            and set(re.findall(r"\.\s*(\w+)\s*\(", prefix)).issubset(
                {"begin", "map_err", "data_plane_id", "acquire_mail_submission_lease"})
        )
        if not (covered or no_op or operational or lease):
            errors.append(f"{source}:{raw.count(chr(10), 0, commit.start()) + 1} commits without audit+outbox or an explicit operational/no-op proof")
    return errors


def session_correlation_errors(source: Path, raw: str) -> list[str]:
    """Request-authorized audits must consider the verified browser session.

    `begin()` maintenance paths have no browser session and are exempt. A
    helper that builds the event still has to call `correlate_human_session`;
    a comment or string cannot satisfy the check.
    """
    production = production_source(raw)
    code = masked(production)
    scopes = functions(code)
    bodies = {item.name: code[item.body_start:item.end] for item in scopes}
    starts = {item.name: item.start for item in scopes}

    def calls(body: str) -> list[str]:
        found = []
        for call in re.finditer(r"\b([A-Za-z_]\w*)\s*\(", body):
            if body[: call.start()].rstrip().endswith((".", ":")):
                continue
            found.append(call[1])
        return found

    def builds_audit(name: str, seen: frozenset[str] = frozenset()) -> bool:
        if name in seen or name not in bodies:
            return False
        body = bodies[name]
        if "AuditEvent::new" in body:
            return True
        return any(builds_audit(callee, seen | {name}) for callee in calls(body))

    errors = []
    reported = set()
    for item in scopes:
        if "begin_authorized" not in bodies[item.name]:
            continue
        stack = [item.name]
        visited = set()
        while stack:
            name = stack.pop()
            if name in visited or name not in bodies:
                continue
            visited.add(name)
            body = bodies[name]
            if (
                "AuditEvent::new" in body
                and "correlate_human_session" not in body
                and name not in reported
            ):
                reported.add(name)
                line = raw.count("\n", 0, starts[name]) + 1
                errors.append(
                    f"{source}:{line} {name} appends a request-authorized audit "
                    "without correlate_human_session"
                )
            for callee in calls(body):
                if builds_audit(callee, frozenset({name})):
                    stack.append(callee)
    return errors


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
    for source in sorted(APPLICATION.rglob("*.rs")):
        if source.name == "tests.rs" or "tests" in source.relative_to(APPLICATION).parts:
            continue
        text = source.read_text(encoding="utf-8")
        visible = masked(production_source(text), keep_strings=True)
        application_actions.update(
            match["action"] for match in ACTION_LITERAL_PATTERN.finditer(visible)
        )
        errors.extend(transaction_coverage_errors(source.relative_to(ROOT), text))
        errors.extend(session_correlation_errors(source.relative_to(ROOT), text))

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
