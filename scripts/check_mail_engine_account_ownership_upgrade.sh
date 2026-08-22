#!/usr/bin/env bash
set -euo pipefail

readonly DATABASE_PREFIX="copilotmail_owner_upgrade_"
readonly DATABASE_NAME_PATTERN='^copilotmail_owner_upgrade_[a-zA-Z0-9_]+$'
readonly SCRIPT_DIR="$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")" && pwd -P)"
readonly REPOSITORY_ROOT="$(cd -- "${SCRIPT_DIR}/.." && pwd -P)"
readonly MIGRATION_DIRECTORY="${REPOSITORY_ROOT}/crates/copilotmail-postgres/migrations"
readonly REQUIRE_READINESS_GATE="${COPILOTMAIL_ACCOUNT_OWNER_REQUIRE_READINESS_GATE:-0}"

usage() {
  cat <<'EOF'
Usage:
  scripts/check_mail_engine_account_ownership_upgrade.sh
  scripts/check_mail_engine_account_ownership_upgrade.sh --self-test

Database connection settings come from libpq environment variables. PGDATABASE
must name an existing administrative database. If cargo, python3, and
COPILOTMAIL_DATABASE_URL are available, the real control-plane `check` command
also verifies trigger and current-binding coverage fail closed. Set
COPILOTMAIL_ACCOUNT_OWNER_REQUIRE_READINESS_GATE=1 to make that check mandatory.
EOF
}

die() {
  printf 'mail-engine account ownership upgrade verification failed: %s\n' \
    "$*" >&2
  exit 1
}

valid_generated_database() {
  local database="${1:-}"
  local source="${2:-}"
  [[ "${database}" =~ ${DATABASE_NAME_PATTERN} ]] &&
    [[ "${database}" != "${DATABASE_PREFIX}" ]] &&
    [[ "${database}" != "${source}" ]] &&
    ((${#database} <= 63))
}

self_test() {
  local source_database="copilotmail"
  local generated_database="copilotmail_owner_upgrade_20260727123000_123_456"

  valid_generated_database "${generated_database}" "${source_database}" ||
    die "self-test rejected a valid generated database"
  if valid_generated_database "${source_database}" "${source_database}"; then
    die "self-test accepted the source database"
  fi
  if valid_generated_database "${DATABASE_PREFIX}" "${source_database}"; then
    die "self-test accepted the prefix-only database"
  fi
  if valid_generated_database \
    'copilotmail_owner_upgrade_fixture;drop_database' \
    "${source_database}"; then
    die "self-test accepted a SQL-shaped database name"
  fi
  if valid_generated_database \
    'copilotmail_owner_upgrade_aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa' \
    "${source_database}"; then
    die "self-test accepted an overlong database name"
  fi

  printf 'mail-engine account ownership upgrade safety self-test passed\n'
}

case "${1:-}" in
  "")
    ;;
  --self-test)
    (($# == 1)) || die "--self-test does not accept additional arguments"
    self_test
    exit 0
    ;;
  --help|-h)
    usage
    exit 0
    ;;
  *)
    usage >&2
    die "unknown argument"
    ;;
esac

for command_name in psql createdb dropdb; do
  command -v "${command_name}" >/dev/null 2>&1 ||
    die "required PostgreSQL client command is unavailable: ${command_name}"
done

[[ -d "${MIGRATION_DIRECTORY}" ]] ||
  die "migration directory is unavailable: ${MIGRATION_DIRECTORY}"

source_database="${PGDATABASE:-}"
[[ -n "${source_database}" ]] ||
  die "PGDATABASE must name the existing administrative connection database"

suffix="$(date -u +%Y%m%d%H%M%S)_$$_${RANDOM}"
positive_database="${DATABASE_PREFIX}${suffix}"
conflict_database="${positive_database}_conflict"
unproven_database="${positive_database}_unproven"

for generated_database in \
  "${positive_database}" \
  "${conflict_database}" \
  "${unproven_database}"; do
  valid_generated_database "${generated_database}" "${source_database}" ||
    die "refusing unsafe generated database name: ${generated_database}"
done

cleanup() {
  local cleanup_database
  for cleanup_database in \
    "${unproven_database}" \
    "${conflict_database}" \
    "${positive_database}"; do
    if [[ "${cleanup_database}" == "${DATABASE_PREFIX}"* ]] &&
       [[ "${cleanup_database}" != "${source_database}" ]]; then
      PGDATABASE="${source_database}" \
        dropdb --if-exists --force -- "${cleanup_database}" >/dev/null 2>&1 || true
    fi
  done
}
trap cleanup EXIT INT TERM

psql_database() {
  local database="$1"
  shift
  PGDATABASE="${database}" psql \
    --no-psqlrc \
    --no-password \
    --set=ON_ERROR_STOP=1 \
    "$@"
}

apply_migrations_through_0050() {
  local database="$1"
  local migration migration_name applied=0

  while IFS= read -r migration; do
    migration_name="$(basename -- "${migration}")"
    [[ "${migration_name}" < "0051_" ]] || break
    psql_database "${database}" \
      --quiet \
      --single-transaction \
      --file="${migration}" >/dev/null ||
      return $?
    applied=$((applied + 1))
  done < <(
    find "${MIGRATION_DIRECTORY}" \
      -maxdepth 1 \
      -type f \
      -name '[0-9][0-9][0-9][0-9]_*.sql' \
      -print |
      LC_ALL=C sort
  )

  [[ "${applied}" == "50" ]] || {
    printf 'expected 50 pre-0051 migrations, applied %s\n' "${applied}" >&2
    exit 1
  }
}

apply_migrations_after_0050() {
  local database="$1"
  local migration migration_name applied=0

  while IFS= read -r migration; do
    migration_name="$(basename -- "${migration}")"
    [[ "${migration_name}" < "0051_" ]] && continue
    psql_database "${database}" \
      --quiet \
      --single-transaction \
      --file="${migration}" >/dev/null ||
      return $?
    applied=$((applied + 1))
  done < <(
    find "${MIGRATION_DIRECTORY}" \
      -maxdepth 1 \
      -type f \
      -name '[0-9][0-9][0-9][0-9]_*.sql' \
      -print |
      LC_ALL=C sort
  )

  ((applied >= 1)) ||
    die "expected at least one post-0050 migration"
}

database_url_for() {
  local database="$1"
  COPILOTMAIL_ACCOUNT_OWNER_TARGET_DATABASE="${database}" python3 - <<'PY'
import os
from urllib.parse import quote, urlsplit, urlunsplit

source = os.environ["COPILOTMAIL_DATABASE_URL"]
target = os.environ["COPILOTMAIL_ACCOUNT_OWNER_TARGET_DATABASE"]
parsed = urlsplit(source)
if parsed.scheme not in {"postgres", "postgresql"} or not parsed.netloc:
    raise SystemExit("COPILOTMAIL_DATABASE_URL is not a network PostgreSQL URL")
print(urlunsplit((
    parsed.scheme,
    parsed.netloc,
    "/" + quote(target, safe=""),
    parsed.query,
    parsed.fragment,
)))
PY
}

readiness_gate_available=0
if command -v cargo >/dev/null 2>&1 &&
   command -v python3 >/dev/null 2>&1 &&
   [[ -n "${COPILOTMAIL_DATABASE_URL:-}" ]]; then
  readiness_gate_available=1
fi
if [[ "${REQUIRE_READINESS_GATE}" == "1" ]] &&
   ((readiness_gate_available == 0)); then
  die "the real readiness gate is required but cargo, python3, or COPILOTMAIL_DATABASE_URL is unavailable"
fi
[[ "${REQUIRE_READINESS_GATE}" =~ ^[01]$ ]] ||
  die "COPILOTMAIL_ACCOUNT_OWNER_REQUIRE_READINESS_GATE must be 0 or 1"

run_readiness_gate() {
  local database="$1"
  local database_url
  database_url="$(database_url_for "${database}")"
  COPILOTMAIL_DATABASE_URL="${database_url}" \
    COPILOTMAIL_ENVIRONMENT=development \
    cargo run \
      --quiet \
      --locked \
      --manifest-path="${REPOSITORY_ROOT}/Cargo.toml" \
      --package=copilotmail-control \
      -- check
}

expect_readiness_rejection() {
  local database="$1"
  local damage="$2"
  local output status

  set +e
  output="$(run_readiness_gate "${database}" 2>&1)"
  status=$?
  set -e
  if ((status == 0)); then
    die "control-plane readiness accepted ${damage}"
  fi
  grep -Fq 'control-plane schema is not migrated' <<<"${output}" ||
    die "control-plane readiness rejected ${damage} for an unexpected reason"
}

PGDATABASE="${source_database}" createdb -- "${positive_database}"
apply_migrations_through_0050 "${positive_database}"

psql_database "${positive_database}" --quiet <<'SQL'
BEGIN;

INSERT INTO organizations(id, slug, display_name, created_at, updated_at) VALUES
    ('10000000-0000-0000-0000-000000000001', 'owner-upgrade-a',
     'Owner Upgrade A', '2026-01-01T00:00:00Z', '2026-01-01T00:00:00Z'),
    ('10000000-0000-0000-0000-000000000002', 'owner-upgrade-b',
     'Owner Upgrade B', '2026-01-01T00:00:00Z', '2026-01-01T00:00:00Z');

INSERT INTO principals(
    id, organization_id, kind, status, display_name, created_at, updated_at
) VALUES
    ('20000000-0000-0000-0000-000000000001',
     '10000000-0000-0000-0000-000000000001',
     'human', 'active', 'Owner A',
     '2026-01-01T00:00:00Z', '2026-01-01T00:00:00Z'),
    ('20000000-0000-0000-0000-000000000002',
     '10000000-0000-0000-0000-000000000002',
     'human', 'active', 'Owner B',
     '2026-01-01T00:00:00Z', '2026-01-01T00:00:00Z');

INSERT INTO mailboxes(
    id, organization_id, kind, primary_address, created_at
) VALUES
    ('30000000-0000-0000-0000-000000000001',
     '10000000-0000-0000-0000-000000000001',
     'human', 'owner-a@example.test', '2026-01-01T00:00:00Z'),
    ('30000000-0000-0000-0000-000000000003',
     '10000000-0000-0000-0000-000000000001',
     'human', 'owner-a-second@example.test', '2026-01-01T00:00:00Z'),
    ('30000000-0000-0000-0000-000000000004',
     '10000000-0000-0000-0000-000000000001',
     'human', 'owner-a-new-binding@example.test', '2026-01-01T00:00:00Z'),
    ('30000000-0000-0000-0000-000000000002',
     '10000000-0000-0000-0000-000000000002',
     'human', 'owner-b@example.test', '2026-01-01T00:00:00Z');

INSERT INTO mail_engine_mailbox_bindings(
    mailbox_id, organization_id, source, account_ref, version,
    updated_by, created_at, updated_at
) VALUES
    ('30000000-0000-0000-0000-000000000001',
     '10000000-0000-0000-0000-000000000001',
     'stalwart', '101', 1,
     '20000000-0000-0000-0000-000000000001',
     '2026-01-01T01:00:00Z', '2026-01-01T01:00:00Z'),
    ('30000000-0000-0000-0000-000000000003',
     '10000000-0000-0000-0000-000000000001',
     'stalwart', '301', 1,
     '20000000-0000-0000-0000-000000000001',
     '2026-01-01T01:00:00Z', '2026-01-01T01:00:00Z'),
    ('30000000-0000-0000-0000-000000000002',
     '10000000-0000-0000-0000-000000000002',
     'stalwart', '201', 1,
     '20000000-0000-0000-0000-000000000002',
     '2026-01-01T01:00:00Z', '2026-01-01T01:00:00Z');

INSERT INTO audit_events(
    id, organization_id, actor_principal_id, action, resource_type,
    resource_id, outcome, trace_id, details, occurred_at,
    received_at, request_id
) VALUES
    ('40000000-0000-0000-0000-000000000001',
     '10000000-0000-0000-0000-000000000001',
     '20000000-0000-0000-0000-000000000001',
     'mail_engine.mailbox_binding.updated', 'mail_engine.mailbox_binding',
     '30000000-0000-0000-0000-000000000001', 'success',
     'owner-upgrade-a-1',
     jsonb_build_object(
         'mailbox_id', '30000000-0000-0000-0000-000000000001',
         'source', 'stalwart', 'account_ref', '101', 'version', 1
     ),
     '2026-01-01T01:00:00Z', '2026-01-01T01:00:00Z',
     'owner-upgrade-a-1'),
    ('40000000-0000-0000-0000-000000000003',
     '10000000-0000-0000-0000-000000000002',
     '20000000-0000-0000-0000-000000000002',
     'mail_engine.mailbox_binding.updated', 'mail_engine.mailbox_binding',
     '30000000-0000-0000-0000-000000000002', 'success',
     'owner-upgrade-b-1',
     jsonb_build_object(
         'mailbox_id', '30000000-0000-0000-0000-000000000002',
         'source', 'stalwart', 'account_ref', '201', 'version', 1
     ),
     '2026-01-01T01:00:00Z', '2026-01-01T01:00:00Z',
     'owner-upgrade-b-1'),
    ('40000000-0000-0000-0000-000000000005',
     '10000000-0000-0000-0000-000000000001',
     '20000000-0000-0000-0000-000000000001',
     'mail_engine.mailbox_binding.updated', 'mail_engine.mailbox_binding',
     '30000000-0000-0000-0000-000000000003', 'success',
     'owner-upgrade-a-3',
     jsonb_build_object(
         'mailbox_id', '30000000-0000-0000-0000-000000000003',
         'source', 'stalwart', 'account_ref', '301', 'version', 1
     ),
     '2026-01-01T01:00:00Z', '2026-01-01T01:00:00Z',
     'owner-upgrade-a-3');

UPDATE mail_engine_mailbox_bindings
   SET account_ref = '102',
       version = 2,
       updated_at = '2026-01-02T01:00:00Z'
 WHERE mailbox_id = '30000000-0000-0000-0000-000000000001';

INSERT INTO audit_events(
    id, organization_id, actor_principal_id, action, resource_type,
    resource_id, outcome, trace_id, details, occurred_at,
    received_at, request_id
) VALUES (
    '40000000-0000-0000-0000-000000000002',
    '10000000-0000-0000-0000-000000000001',
    '20000000-0000-0000-0000-000000000001',
    'mail_engine.mailbox_binding.updated', 'mail_engine.mailbox_binding',
    '30000000-0000-0000-0000-000000000001', 'success',
    'owner-upgrade-a-2',
    jsonb_build_object(
        'mailbox_id', '30000000-0000-0000-0000-000000000001',
        'source', 'stalwart', 'account_ref', '102', 'version', 2
    ),
    '2026-01-02T01:00:00Z', '2026-01-02T01:00:00Z',
    'owner-upgrade-a-2'
);

COMMIT;
SQL

audit_evidence_count="$(
  psql_database "${positive_database}" \
    --quiet \
    --tuples-only \
    --no-align \
    --command="
      SELECT count(*)
        FROM audit_events
       WHERE action = 'mail_engine.mailbox_binding.updated'
         AND resource_type = 'mail_engine.mailbox_binding'
         AND outcome = 'success'
         AND details ?& ARRAY[
             'mailbox_id', 'source', 'account_ref', 'version'
         ]
         AND details ->> 'mailbox_id' = resource_id
         AND details ->> 'source' = 'stalwart'
         AND details ->> 'account_ref' ~ '^(0|[1-9][0-9]{0,19})$'
         AND jsonb_typeof(details -> 'version') = 'number';
    "
)"
[[ "${audit_evidence_count}" == "4" ]] ||
  die "the pre-upgrade fixture does not match the production audit evidence shape"

PGDATABASE="${source_database}" \
  createdb --template="${positive_database}" -- "${conflict_database}"
PGDATABASE="${source_database}" \
  createdb --template="${positive_database}" -- "${unproven_database}"

apply_migrations_after_0050 "${positive_database}"

owner_count="$(
  psql_database "${positive_database}" \
    --quiet \
    --tuples-only \
    --no-align \
    --command="
      SELECT count(*)
        FROM mail_engine_account_owners
       WHERE (source, account_ref, organization_id, mailbox_id) IN (
           ('stalwart', '101',
            '10000000-0000-0000-0000-000000000001'::uuid,
            '30000000-0000-0000-0000-000000000001'::uuid),
           ('stalwart', '102',
            '10000000-0000-0000-0000-000000000001'::uuid,
            '30000000-0000-0000-0000-000000000001'::uuid),
           ('stalwart', '201',
            '10000000-0000-0000-0000-000000000002'::uuid,
            '30000000-0000-0000-0000-000000000002'::uuid),
           ('stalwart', '301',
            '10000000-0000-0000-0000-000000000001'::uuid,
            '30000000-0000-0000-0000-000000000003'::uuid)
       );
    "
)"
[[ "${owner_count}" == "4" ]] ||
  die "post-0050 migrations did not preserve current and retired account owners"

retired_owner_count="$(
  psql_database "${positive_database}" \
    --quiet \
    --tuples-only \
    --no-align \
    --command="
      SELECT count(*)
        FROM mail_engine_account_owners AS owner
        LEFT JOIN mail_engine_mailbox_bindings AS binding
          ON binding.source = owner.source
         AND binding.account_ref = owner.account_ref
       WHERE owner.account_ref = '101'
         AND binding.account_ref IS NULL;
    "
)"
[[ "${retired_owner_count}" == "1" ]] ||
  die "post-0050 migrations did not retain the retired historical account"

current_binding_coverage_gaps="$(
  psql_database "${positive_database}" \
    --quiet \
    --tuples-only \
    --no-align \
    --command="
      SELECT count(*)
        FROM mail_engine_mailbox_bindings AS binding
        LEFT JOIN mail_engine_account_owners AS owner
          ON owner.source = binding.source
         AND owner.account_ref = binding.account_ref
       WHERE owner.account_ref IS NULL
          OR owner.organization_id <> binding.organization_id
          OR owner.mailbox_id <> binding.mailbox_id;
    "
)"
[[ "${current_binding_coverage_gaps}" == "0" ]] ||
  die "post-0050 migrations left a current binding ownership gap"

set +e
orphan_insert_output="$(
  psql_database "${positive_database}" \
    --quiet \
    --set=VERBOSITY=verbose \
    2>&1 <<'SQL'
BEGIN;

INSERT INTO mail_engine_account_owners(
    source,
    account_ref,
    organization_id,
    mailbox_id,
    first_bound_at
)
VALUES (
    'stalwart',
    '999',
    '10000000-0000-0000-0000-000000000001',
    '30000000-0000-0000-0000-000000000004',
    '2026-01-03T00:00:00Z'
);

COMMIT;
SQL
)"
orphan_insert_status=$?
set -e
if ((orphan_insert_status == 0)); then
  die "an owner without a current binding committed successfully"
fi
grep -Fq 'mail_engine_account_owner_insert_without_binding' \
  <<<"${orphan_insert_output}" ||
  die "orphan owner INSERT failed without the governed constraint"
orphan_owner_count="$(
  psql_database "${positive_database}" \
    --quiet \
    --tuples-only \
    --no-align \
    --command="
      SELECT count(*)
        FROM mail_engine_account_owners
       WHERE source = 'stalwart'
         AND account_ref = '999';
    "
)"
[[ "${orphan_owner_count}" == "0" ]] ||
  die "the commit-time orphan owner rejection was not atomic"

psql_database "${positive_database}" --quiet <<'SQL'
BEGIN;

INSERT INTO mail_engine_mailbox_bindings(
    mailbox_id,
    organization_id,
    source,
    account_ref,
    version,
    updated_by,
    created_at,
    updated_at
)
VALUES (
    '30000000-0000-0000-0000-000000000004',
    '10000000-0000-0000-0000-000000000001',
    'stalwart',
    '401',
    1,
    '20000000-0000-0000-0000-000000000001',
    '2026-01-03T00:00:00Z',
    '2026-01-03T00:00:00Z'
);

COMMIT;

BEGIN;

UPDATE mail_engine_mailbox_bindings
   SET account_ref = '402',
       version = version + 1,
       updated_at = '2026-01-03T00:30:00Z'
 WHERE mailbox_id = '30000000-0000-0000-0000-000000000004';

COMMIT;
SQL

normal_binding_owner_count="$(
  psql_database "${positive_database}" \
    --quiet \
    --tuples-only \
    --no-align \
    --command="
      SELECT count(*)
        FROM mail_engine_account_owners
       WHERE (source, account_ref, organization_id, mailbox_id) IN (
           ('stalwart', '401',
            '10000000-0000-0000-0000-000000000001'::uuid,
            '30000000-0000-0000-0000-000000000004'::uuid),
           ('stalwart', '402',
            '10000000-0000-0000-0000-000000000001'::uuid,
            '30000000-0000-0000-0000-000000000004'::uuid)
       );
    "
)"
[[ "${normal_binding_owner_count}" == "2" ]] ||
  die "normal first binding or account rotation did not claim permanent ownership"
current_rotated_binding_count="$(
  psql_database "${positive_database}" \
    --quiet \
    --tuples-only \
    --no-align \
    --command="
      SELECT count(*)
        FROM mail_engine_mailbox_bindings
       WHERE mailbox_id =
             '30000000-0000-0000-0000-000000000004'
         AND organization_id =
             '10000000-0000-0000-0000-000000000001'
         AND source = 'stalwart'
         AND account_ref = '402'
         AND version = 2;
    "
)"
[[ "${current_rotated_binding_count}" == "1" ]] ||
  die "normal account rotation did not commit its current binding"

psql_database "${positive_database}" --quiet <<'SQL'
DO $$
DECLARE
    rejected_constraint text;
BEGIN
    BEGIN
        UPDATE mail_engine_mailbox_bindings
           SET account_ref = '101',
               version = version + 1,
               updated_at = '2026-01-03T01:00:00Z'
         WHERE mailbox_id = '30000000-0000-0000-0000-000000000002';
        RAISE EXCEPTION 'cross-tenant account reassignment unexpectedly succeeded';
    EXCEPTION
        WHEN check_violation THEN
            GET STACKED DIAGNOSTICS
                rejected_constraint = CONSTRAINT_NAME;
            IF rejected_constraint IS DISTINCT FROM
                'mail_engine_account_owner_mismatch'
            THEN
                RAISE EXCEPTION
                    'unexpected reassignment constraint: %',
                    rejected_constraint;
            END IF;
    END;

    BEGIN
        UPDATE mail_engine_mailbox_bindings
           SET account_ref = '101',
               version = version + 1,
               updated_at = '2026-01-03T01:00:00Z'
         WHERE mailbox_id = '30000000-0000-0000-0000-000000000003';
        RAISE EXCEPTION
            'same-tenant cross-mailbox account reassignment unexpectedly succeeded';
    EXCEPTION
        WHEN check_violation THEN
            GET STACKED DIAGNOSTICS
                rejected_constraint = CONSTRAINT_NAME;
            IF rejected_constraint IS DISTINCT FROM
                'mail_engine_account_owner_mismatch'
            THEN
                RAISE EXCEPTION
                    'unexpected same-tenant reassignment constraint: %',
                    rejected_constraint;
            END IF;
    END;
END;
$$;

UPDATE mail_engine_mailbox_bindings
   SET account_ref = '101',
       version = version + 1,
       updated_at = '2026-01-03T01:00:00Z'
 WHERE mailbox_id = '30000000-0000-0000-0000-000000000001';
SQL

if ((readiness_gate_available == 1)); then
  run_readiness_gate "${positive_database}" >/dev/null 2>&1 ||
    die "control-plane readiness rejected the intact upgraded fixture"

  psql_database "${positive_database}" --quiet --command="
    ALTER TABLE mail_engine_mailbox_bindings
        DISABLE TRIGGER mail_engine_mailbox_bindings_enforce_account_owner;
  "
  expect_readiness_rejection \
    "${positive_database}" \
    "a disabled account-owner enforcement trigger"
  psql_database "${positive_database}" --quiet --command="
    ALTER TABLE mail_engine_mailbox_bindings
        ENABLE TRIGGER mail_engine_mailbox_bindings_enforce_account_owner;
  "

  psql_database "${positive_database}" --quiet --command="
    DROP TRIGGER mail_engine_account_owners_reject_update_delete
        ON mail_engine_account_owners;
  "
  expect_readiness_rejection \
    "${positive_database}" \
    "a deleted account-owner update/delete trigger"
  psql_database "${positive_database}" --quiet --command="
    CREATE TRIGGER mail_engine_account_owners_reject_update_delete
    BEFORE UPDATE OR DELETE ON mail_engine_account_owners
    FOR EACH ROW
    EXECUTE FUNCTION reject_mail_engine_account_owner_mutation();
  "

  psql_database "${positive_database}" --quiet --command="
    DROP TRIGGER mail_engine_account_owners_reject_truncate
        ON mail_engine_account_owners;
  "
  expect_readiness_rejection \
    "${positive_database}" \
    "a deleted account-owner truncate trigger"
  psql_database "${positive_database}" --quiet --command="
    CREATE TRIGGER mail_engine_account_owners_reject_truncate
    BEFORE TRUNCATE ON mail_engine_account_owners
    FOR EACH STATEMENT
    EXECUTE FUNCTION reject_mail_engine_account_owner_mutation();
  "

  psql_database "${positive_database}" --quiet --command="
    DROP TRIGGER mail_engine_account_owners_require_current_binding
        ON mail_engine_account_owners;
  "
  expect_readiness_rejection \
    "${positive_database}" \
    "a deleted account-owner current-binding constraint trigger"
  psql_database "${positive_database}" --quiet --command="
    CREATE CONSTRAINT TRIGGER
        mail_engine_account_owners_require_current_binding
    AFTER INSERT ON mail_engine_account_owners
    DEFERRABLE INITIALLY DEFERRED
    FOR EACH ROW
    EXECUTE FUNCTION require_current_binding_for_account_owner();
  "

  psql_database "${positive_database}" --quiet <<'SQL'
ALTER TABLE mail_engine_mailbox_bindings
    DISABLE TRIGGER mail_engine_mailbox_bindings_enforce_account_owner;

UPDATE mail_engine_mailbox_bindings
   SET account_ref = '302',
       version = version + 1,
       updated_at = '2026-01-04T01:00:00Z'
 WHERE mailbox_id = '30000000-0000-0000-0000-000000000003';

ALTER TABLE mail_engine_mailbox_bindings
    ENABLE TRIGGER mail_engine_mailbox_bindings_enforce_account_owner;
SQL
  coverage_damage_count="$(
    psql_database "${positive_database}" \
      --quiet \
      --tuples-only \
      --no-align \
      --command="
        SELECT count(*)
          FROM mail_engine_mailbox_bindings AS binding
          LEFT JOIN mail_engine_account_owners AS owner
            ON owner.source = binding.source
           AND owner.account_ref = binding.account_ref
         WHERE owner.account_ref IS NULL
            OR owner.organization_id <> binding.organization_id
            OR owner.mailbox_id <> binding.mailbox_id;
      "
  )"
  [[ "${coverage_damage_count}" == "1" ]] ||
    die "failed to create exactly one current-binding coverage defect"
  expect_readiness_rejection \
    "${positive_database}" \
    "a current binding without exact owner coverage"
  psql_database "${positive_database}" --quiet --command="
    INSERT INTO mail_engine_account_owners(
        source,
        account_ref,
        organization_id,
        mailbox_id,
        first_bound_at
    )
    VALUES (
        'stalwart',
        '302',
        '10000000-0000-0000-0000-000000000001',
        '30000000-0000-0000-0000-000000000003',
        '2026-01-04T01:00:00Z'
    );
  "
  repaired_coverage_gaps="$(
    psql_database "${positive_database}" \
      --quiet \
      --tuples-only \
      --no-align \
      --command="
        SELECT count(*)
          FROM mail_engine_mailbox_bindings AS binding
          LEFT JOIN mail_engine_account_owners AS owner
            ON owner.source = binding.source
           AND owner.account_ref = binding.account_ref
         WHERE owner.account_ref IS NULL
            OR owner.organization_id <> binding.organization_id
            OR owner.mailbox_id <> binding.mailbox_id;
      "
  )"
  [[ "${repaired_coverage_gaps}" == "0" ]] ||
    die "exact owner insertion did not repair current-binding coverage"

  run_readiness_gate "${positive_database}" >/dev/null 2>&1 ||
    die "control-plane readiness did not recover after exact gate repairs"
else
  printf '%s\n' \
    'mail-engine account ownership: real control-plane readiness gate skipped (cargo, python3, or COPILOTMAIL_DATABASE_URL unavailable)' \
    >&2
fi

psql_database "${conflict_database}" --quiet <<'SQL'
BEGIN;

UPDATE mail_engine_mailbox_bindings
   SET account_ref = '101',
       version = 2,
       updated_at = '2026-01-03T01:00:00Z'
 WHERE mailbox_id = '30000000-0000-0000-0000-000000000002';

INSERT INTO audit_events(
    id, organization_id, actor_principal_id, action, resource_type,
    resource_id, outcome, trace_id, details, occurred_at,
    received_at, request_id
) VALUES (
    '40000000-0000-0000-0000-000000000004',
    '10000000-0000-0000-0000-000000000002',
    '20000000-0000-0000-0000-000000000002',
    'mail_engine.mailbox_binding.updated', 'mail_engine.mailbox_binding',
    '30000000-0000-0000-0000-000000000002', 'success',
    'owner-upgrade-conflict',
    jsonb_build_object(
        'mailbox_id', '30000000-0000-0000-0000-000000000002',
        'source', 'stalwart', 'account_ref', '101', 'version', 2
    ),
    '2026-01-03T01:00:00Z', '2026-01-03T01:00:00Z',
    'owner-upgrade-conflict'
);

COMMIT;
SQL

set +e
conflict_output="$(
  apply_migrations_after_0050 "${conflict_database}" 2>&1
)"
conflict_status=$?
set -e

if [[ "${conflict_status}" == "0" ]]; then
  die "post-0050 migrations accepted ambiguous historical account ownership"
fi
grep -Fq 'historical mail-engine account ownership is ambiguous' \
  <<<"${conflict_output}" ||
  die "post-0050 migrations failed for an unexpected conflict reason"

rolled_back_objects="$(
  psql_database "${conflict_database}" \
    --quiet \
    --tuples-only \
    --no-align \
    --command="
      SELECT concat_ws(
          ',',
          to_regclass('public.mail_engine_account_owners')::text,
          to_regprocedure(
              'public.enforce_mail_engine_account_owner()'
          )::text,
          to_regprocedure(
              'public.reject_mail_engine_account_owner_mutation()'
          )::text,
          to_regprocedure(
              'public.require_current_binding_for_account_owner()'
          )::text
      );
    "
)"
[[ -z "${rolled_back_objects}" ]] ||
  die "failed ownership migration left partial schema objects behind"

psql_database "${unproven_database}" \
  --quiet \
  --single-transaction \
  --file="${MIGRATION_DIRECTORY}/0051_mail_engine_account_ownership.sql" \
  >/dev/null

psql_database "${unproven_database}" --quiet --command="
  INSERT INTO mail_engine_account_owners(
      source,
      account_ref,
      organization_id,
      mailbox_id,
      first_bound_at
  )
  VALUES (
      'stalwart',
      '888',
      '10000000-0000-0000-0000-000000000001',
      '30000000-0000-0000-0000-000000000004',
      '2026-01-03T00:00:00Z'
  );
"

unproven_fixture_before="$(
  psql_database "${unproven_database}" \
    --quiet \
    --tuples-only \
    --no-align \
    --command="
      SELECT md5(
          jsonb_build_object(
              'owners',
              COALESCE(
                  (
                      SELECT jsonb_agg(
                          to_jsonb(owner_row)
                          ORDER BY owner_row.source, owner_row.account_ref
                      )
                        FROM mail_engine_account_owners AS owner_row
                  ),
                  '[]'::jsonb
              ),
              'bindings',
              COALESCE(
                  (
                      SELECT jsonb_agg(
                          to_jsonb(binding_row)
                          ORDER BY binding_row.mailbox_id
                      )
                        FROM mail_engine_mailbox_bindings AS binding_row
                  ),
                  '[]'::jsonb
              ),
              'binding_audit',
              COALESCE(
                  (
                      SELECT jsonb_agg(
                          to_jsonb(event_row)
                          ORDER BY event_row.id
                      )
                        FROM audit_events AS event_row
                       WHERE event_row.action =
                             'mail_engine.mailbox_binding.updated'
                  ),
                  '[]'::jsonb
              )
          )::text
      );
    "
)"

set +e
unproven_output="$(
  psql_database "${unproven_database}" \
    --quiet \
    --set=VERBOSITY=verbose \
    --single-transaction \
    --file="${MIGRATION_DIRECTORY}/0052_mail_engine_account_owner_insert_guard.sql" \
    2>&1
)"
unproven_status=$?
set -e
if ((unproven_status == 0)); then
  die "0052 accepted a persisted owner without trusted evidence"
fi
grep -Fq 'mail_engine_account_owner_unproven' \
  <<<"${unproven_output}" ||
  die "0052 rejected the unproven owner with an unexpected constraint"

unproven_fixture_after="$(
  psql_database "${unproven_database}" \
    --quiet \
    --tuples-only \
    --no-align \
    --command="
      SELECT md5(
          jsonb_build_object(
              'owners',
              COALESCE(
                  (
                      SELECT jsonb_agg(
                          to_jsonb(owner_row)
                          ORDER BY owner_row.source, owner_row.account_ref
                      )
                        FROM mail_engine_account_owners AS owner_row
                  ),
                  '[]'::jsonb
              ),
              'bindings',
              COALESCE(
                  (
                      SELECT jsonb_agg(
                          to_jsonb(binding_row)
                          ORDER BY binding_row.mailbox_id
                      )
                        FROM mail_engine_mailbox_bindings AS binding_row
                  ),
                  '[]'::jsonb
              ),
              'binding_audit',
              COALESCE(
                  (
                      SELECT jsonb_agg(
                          to_jsonb(event_row)
                          ORDER BY event_row.id
                      )
                        FROM audit_events AS event_row
                       WHERE event_row.action =
                             'mail_engine.mailbox_binding.updated'
                  ),
                  '[]'::jsonb
              )
          )::text
      );
    "
)"
[[ "${unproven_fixture_after}" == "${unproven_fixture_before}" ]] ||
  die "failed 0052 changed the pre-migration ownership fixture"

unproven_owner_count="$(
  psql_database "${unproven_database}" \
    --quiet \
    --tuples-only \
    --no-align \
    --command="
      SELECT count(*)
        FROM mail_engine_account_owners
       WHERE source = 'stalwart'
         AND account_ref = '888'
         AND organization_id =
             '10000000-0000-0000-0000-000000000001'
         AND mailbox_id =
             '30000000-0000-0000-0000-000000000004'
         AND first_bound_at = '2026-01-03T00:00:00Z';
    "
)"
[[ "${unproven_owner_count}" == "1" ]] ||
  die "failed 0052 did not preserve the original unproven owner fixture"

post_0052_objects_absent="$(
  psql_database "${unproven_database}" \
    --quiet \
    --tuples-only \
    --no-align \
    --command="
      SELECT (
          to_regprocedure(
              'public.require_current_binding_for_account_owner()'
          ) IS NULL
          AND NOT EXISTS (
              SELECT 1
                FROM pg_trigger
               WHERE tgname =
                     'mail_engine_account_owners_require_current_binding'
          )
          AND NOT EXISTS (
              SELECT 1
                FROM pg_constraint
               WHERE conname =
                     'mail_engine_account_owners_require_current_binding'
          )
      )::int;
    "
)"
[[ "${post_0052_objects_absent}" == "1" ]] ||
  die "failed 0052 left its function or constraint trigger behind"

post_0051_objects_intact="$(
  psql_database "${unproven_database}" \
    --quiet \
    --tuples-only \
    --no-align \
    --command="
      SELECT (
          to_regclass('public.mail_engine_account_owners') IS NOT NULL
          AND to_regprocedure(
              'public.enforce_mail_engine_account_owner()'
          ) IS NOT NULL
          AND to_regprocedure(
              'public.reject_mail_engine_account_owner_mutation()'
          ) IS NOT NULL
          AND EXISTS (
              SELECT 1
                FROM pg_trigger
               WHERE tgrelid =
                     'public.mail_engine_mailbox_bindings'::regclass
                 AND tgname =
                     'mail_engine_mailbox_bindings_enforce_account_owner'
                 AND NOT tgisinternal
                 AND tgenabled = 'O'
                 AND tgtype = 23
                 AND tgfoid =
                     'public.enforce_mail_engine_account_owner()'::regprocedure
          )
          AND EXISTS (
              SELECT 1
                FROM pg_trigger
               WHERE tgrelid =
                     'public.mail_engine_account_owners'::regclass
                 AND tgname =
                     'mail_engine_account_owners_reject_update_delete'
                 AND NOT tgisinternal
                 AND tgenabled = 'O'
                 AND tgtype = 27
                 AND tgfoid =
                     'public.reject_mail_engine_account_owner_mutation()'::regprocedure
          )
          AND EXISTS (
              SELECT 1
                FROM pg_trigger
               WHERE tgrelid =
                     'public.mail_engine_account_owners'::regclass
                 AND tgname =
                     'mail_engine_account_owners_reject_truncate'
                 AND NOT tgisinternal
                 AND tgenabled = 'O'
                 AND tgtype = 34
                 AND tgfoid =
                     'public.reject_mail_engine_account_owner_mutation()'::regprocedure
          )
      )::int;
    "
)"
[[ "${post_0051_objects_intact}" == "1" ]] ||
  die "failed 0052 damaged objects committed by 0051"

printf 'mail-engine account ownership upgrade verification passed\n'
