#!/usr/bin/env bash
set -Eeuo pipefail
shopt -s inherit_errexit

# Fail-closed logical backup and isolated-restore verification for the V2
# control-plane PostgreSQL database. This script never accepts a caller-chosen
# restore database name.

umask 077

readonly RESTORE_PREFIX="copilotmail_restore_verify_"
readonly RESTORE_NAME_PATTERN='^copilotmail_restore_verify_[0-9]{14}_[0-9a-f]{16}$'
readonly UUID_PATTERN='^[0-9a-fA-F]{8}-[0-9a-fA-F]{4}-[0-9a-fA-F]{4}-[0-9a-fA-F]{4}-[0-9a-fA-F]{12}$'

ARCHIVE_PATH=""
VERIFY_ORGANIZATION_ID=""
SELF_TEST=0
SOURCE_DATABASE=""
MAINTENANCE_DATABASE=""
RESTORE_DATABASE=""
RESTORE_MARKER=""
RESTORE_CREATED=0
PARTIAL_ARCHIVE=""
PARTIAL_EVIDENCE=""
FINAL_ARCHIVE_CREATED=0
FINAL_EVIDENCE_CREATED=0
PUBLICATION_COMPLETE=0
ORIGINAL_PGOPTIONS="${PGOPTIONS:-}"

usage() {
  cat <<'EOF'
Usage:
  scripts/check_postgres_backup_restore.sh --self-test
  scripts/check_postgres_backup_restore.sh \
    --archive /absolute/private/path/control-plane.dump \
    --verify-organization-id UUID

Database connection settings come only from libpq environment variables,
PGSERVICE/PGSERVICEFILE, or PGPASSFILE. PGDATABASE must be the plain source
database name. Never put a password in this command line.
EOF
}

die() {
  printf 'postgres backup/restore gate failed: %s\n' "$*" >&2
  exit 1
}

info() {
  printf 'postgres backup/restore: %s\n' "$*" >&2
}

valid_database_name() {
  local value="${1:-}"
  [[ "${value}" =~ ^[A-Za-z_][A-Za-z0-9_]{0,62}$ ]]
}

valid_restore_database_name() {
  local value="${1:-}"
  [[ "${value}" =~ ${RESTORE_NAME_PATTERN} ]] && ((${#value} <= 63))
}

restore_target_is_safe() {
  local target="${1:-}"
  local source="${2:-}"
  local maintenance="${3:-}"
  valid_restore_database_name "${target}" &&
    [[ "${target}" != "${source}" && "${target}" != "${maintenance}" ]]
}

valid_uuid() {
  local value="${1:-}"
  [[ "${value}" =~ ${UUID_PATTERN} ]]
}

archive_destination_is_safe() {
  local path="${1:-}"
  local directory mode owner mode_value
  [[ "${path}" == /* && "${path}" != *$'\n'* ]] || return 1
  [[ ! -e "${path}" && ! -L "${path}" ]] || return 1
  [[ ! -e "${path}.evidence" && ! -L "${path}.evidence" ]] || return 1
  directory="$(dirname -- "${path}")"
  [[ -d "${directory}" && ! -L "${directory}" ]] || return 1
  owner="$(stat -c '%u' -- "${directory}" 2>/dev/null)" || return 1
  [[ "${owner}" == "$(id -u)" ]] || return 1
  mode="$(stat -c '%a' -- "${directory}" 2>/dev/null)" || return 1
  [[ "${mode}" =~ ^[0-7]{3,4}$ ]] || return 1
  mode_value=$((8#${mode}))
  (((mode_value & 077) == 0))
}

assert_accepts() {
  local label="$1"
  shift
  "$@" || die "self-test unexpectedly rejected ${label}"
}

assert_rejects() {
  local label="$1"
  shift
  if "$@"; then
    die "self-test unexpectedly accepted ${label}"
  fi
}

self_test() {
  local source="copilotmail"
  local maintenance="postgres"
  local valid_target="copilotmail_restore_verify_20260727091530_0123456789abcdef"
  local private_directory new_archive

  assert_accepts "generated restore database" \
    restore_target_is_safe "${valid_target}" "${source}" "${maintenance}"
  assert_rejects "generated target equal to source database" \
    restore_target_is_safe "${valid_target}" "${valid_target}" "${maintenance}"
  assert_rejects "generated target equal to maintenance database" \
    restore_target_is_safe "${valid_target}" "${source}" "${valid_target}"
  assert_rejects "caller-selected unprefixed database" \
    restore_target_is_safe "customer_production" "${source}" "${maintenance}"
  assert_rejects "prefix-only database" \
    restore_target_is_safe "${RESTORE_PREFIX}" "${source}" "${maintenance}"
  assert_rejects "SQL-shaped database name" \
    restore_target_is_safe \
    "copilotmail_restore_verify_20260727091530_deadbeef;drop" \
    "${source}" "${maintenance}"
  assert_accepts "verification UUID" \
    valid_uuid "018f13b9-1111-7bc8-965d-48a28866f073"
  assert_rejects "provider message identifier" valid_uuid "jmap-email-id"

  private_directory="$(mktemp -d)"
  chmod 0700 "${private_directory}"
  new_archive="${private_directory}/control-plane.dump"
  assert_accepts "new archive in a private directory" \
    archive_destination_is_safe "${new_archive}"
  : >"${new_archive}"
  assert_rejects "pre-existing archive" \
    archive_destination_is_safe "${new_archive}"
  rm -f -- "${new_archive}"
  : >"${new_archive}.evidence"
  assert_rejects "pre-existing evidence sidecar" \
    archive_destination_is_safe "${new_archive}"
  rm -f -- "${new_archive}.evidence"
  chmod 0750 "${private_directory}"
  assert_rejects "group-accessible archive directory" \
    archive_destination_is_safe "${new_archive}"
  chmod 0700 "${private_directory}"
  rmdir -- "${private_directory}"

  printf 'PostgreSQL backup/restore safety self-test passed without a database.\n'
}

while (($# > 0)); do
  case "$1" in
    --archive)
      (($# >= 2)) || die "--archive requires a path"
      ARCHIVE_PATH="$2"
      shift 2
      ;;
    --verify-organization-id)
      (($# >= 2)) || die "--verify-organization-id requires a UUID"
      VERIFY_ORGANIZATION_ID="$2"
      shift 2
      ;;
    --self-test)
      SELF_TEST=1
      shift
      ;;
    --help|-h)
      usage
      exit 0
      ;;
    *)
      die "unknown argument"
      ;;
  esac
done

if ((SELF_TEST)); then
  [[ -z "${ARCHIVE_PATH}" && -z "${VERIFY_ORGANIZATION_ID}" ]] ||
    die "--self-test cannot be combined with database arguments"
  self_test
  exit 0
fi

[[ -n "${ARCHIVE_PATH}" ]] || die "--archive is required"
valid_uuid "${VERIFY_ORGANIZATION_ID}" ||
  die "--verify-organization-id must be a UUID"
archive_destination_is_safe "${ARCHIVE_PATH}" ||
  die "archive path must be new, absolute, and inside an owner-only directory"

for command_name in \
  psql pg_dump pg_restore sha256sum mktemp od stat awk id \
  chmod date dirname ln rm rmdir tr; do
  command -v "${command_name}" >/dev/null 2>&1 ||
    die "required command is unavailable: ${command_name}"
done

SOURCE_DATABASE="${PGDATABASE:-}"
MAINTENANCE_DATABASE="${COPILOTMAIL_RESTORE_MAINTENANCE_DATABASE:-postgres}"
valid_database_name "${SOURCE_DATABASE}" ||
  die "PGDATABASE must be an explicit plain database name"
valid_database_name "${MAINTENANCE_DATABASE}" ||
  die "COPILOTMAIL_RESTORE_MAINTENANCE_DATABASE must be a plain database name"
[[ "${SOURCE_DATABASE}" != "${MAINTENANCE_DATABASE}" ]] ||
  die "source and maintenance databases must differ"

random_hex() {
  local byte_count="$1"
  od -An -N"${byte_count}" -tx1 /dev/urandom | tr -d ' \n'
}

RESTORE_DATABASE="$(
  printf '%s%s_%s' \
    "${RESTORE_PREFIX}" \
    "$(date -u +%Y%m%d%H%M%S)" \
    "$(random_hex 8)"
)"
RESTORE_MARKER="copilotmail-restore-verify:$(random_hex 16)"
restore_target_is_safe \
  "${RESTORE_DATABASE}" "${SOURCE_DATABASE}" "${MAINTENANCE_DATABASE}" ||
  die "generated restore database failed the safety policy"

base_pgoptions() {
  printf '%s' \
    "${ORIGINAL_PGOPTIONS:+${ORIGINAL_PGOPTIONS} }-c search_path=pg_catalog,public"
}

readonly_source_pgoptions() {
  printf '%s' \
    "$(base_pgoptions) -c default_transaction_read_only=on"
}

run_psql() {
  local database="$1"
  local read_only="$2"
  local options
  options="$(base_pgoptions)"
  shift 2
  if [[ "${read_only}" == "1" ]]; then
    options="$(readonly_source_pgoptions)"
  fi
  PGDATABASE="${database}" PGOPTIONS="${options}" \
    psql \
      --no-psqlrc \
      --no-password \
      --set=ON_ERROR_STOP=1 \
      --quiet \
      --tuples-only \
      --no-align \
      --field-separator=$'\t' \
      "$@"
}

sha256_text() {
  printf '%s' "$1" | sha256sum | awk '{print $1}'
}

collect_database_attributes() {
  local connection_database="$1"
  local inspected_database="$2"

  run_psql "${connection_database}" 1 \
    --set=inspected_database="${inspected_database}" <<'SQL'
SELECT database_row.encoding::text,
       database_row.datlocprovider::text,
       database_row.datcollate,
       database_row.datctype,
       COALESCE(
         pg_catalog.to_jsonb(database_row) ->> 'datlocale',
         pg_catalog.to_jsonb(database_row) ->> 'daticulocale',
         ''
       ),
       COALESCE(
         pg_catalog.to_jsonb(database_row) ->> 'daticurules',
         ''
       ),
       COALESCE(database_row.datcollversion, '')
  FROM pg_catalog.pg_database AS database_row
 WHERE database_row.datname = :'inspected_database';
SQL
}

collect_database_creation_attributes() {
  local connection_database="$1"
  local inspected_database="$2"

  run_psql "${connection_database}" 1 \
    --set=inspected_database="${inspected_database}" <<'SQL'
SELECT database_row.encoding::text,
       database_row.datlocprovider::text,
       database_row.datcollate,
       database_row.datctype,
       COALESCE(
         pg_catalog.to_jsonb(database_row) ->> 'datlocale',
         pg_catalog.to_jsonb(database_row) ->> 'daticulocale',
         ''
       ),
       COALESCE(
         pg_catalog.to_jsonb(database_row) ->> 'daticurules',
         ''
       )
  FROM pg_catalog.pg_database AS database_row
 WHERE database_row.datname = :'inspected_database';
SQL
}

collect_evidence() {
  local database="$1"
  local database_attributes database_attributes_digest
  local migration_summary migration_count migration_max migration_failed
  local migration_rows migration_digest
  local catalog_count catalog_rows catalog_digest
  local trigger_summary trigger_count trigger_missing trigger_rows trigger_digest
  local organization_count organization_row organization_digest
  local audit_summary audit_count audit_max_sequence audit_head

  database_attributes="$(
    collect_database_attributes "${database}" "${database}"
  )" || die "failed to read database encoding or locale attributes"
  [[ -n "${database_attributes}" ]] ||
    die "database encoding or locale attributes are unreadable"
  database_attributes_digest="$(sha256_text "${database_attributes}")"

  migration_summary="$(
    run_psql "${database}" 1 --command="
      SELECT count(*)::text,
             COALESCE(max(version), 0)::text,
             count(*) FILTER (WHERE NOT success)::text
        FROM \"public\".\"_sqlx_migrations\";
    "
  )" || die "failed to read the SQLx migration summary"
  IFS=$'\t' read -r migration_count migration_max migration_failed \
    <<<"${migration_summary}"
  [[ "${migration_count}" =~ ^[0-9]+$ ]] && ((migration_count > 0)) ||
    die "migration ledger is empty or unreadable in verification database"
  [[ "${migration_max}" =~ ^[0-9]+$ ]] ||
    die "migration maximum version is unreadable"
  [[ "${migration_failed}" == "0" ]] ||
    die "migration ledger contains unsuccessful rows"
  migration_rows="$(
    run_psql "${database}" 1 --command="
      SELECT version, description, success, encode(checksum, 'hex')
        FROM \"public\".\"_sqlx_migrations\"
       ORDER BY version;
    "
  )" || die "failed to read the SQLx migration ledger"
  migration_digest="$(sha256_text "${migration_rows}")"

  catalog_count="$(
    run_psql "${database}" 1 \
      --command="SELECT count(*)::text FROM public.audit_event_catalog;"
  )" || die "failed to read the audit event catalog count"
  [[ "${catalog_count}" =~ ^[0-9]+$ ]] && ((catalog_count > 0)) ||
    die "audit event catalog is empty or unreadable"
  catalog_rows="$(
    run_psql "${database}" 1 --command="
      SELECT action, resource_type, risk, privileged
        FROM public.audit_event_catalog
       ORDER BY action;
    "
  )" || die "failed to read the audit event catalog"
  catalog_digest="$(sha256_text "${catalog_rows}")"

  trigger_summary="$(
    run_psql "${database}" 1 --command="
      WITH expected(table_name, trigger_name) AS (
        VALUES
          ('audit_events', 'audit_events_append_only'),
          ('audit_events', 'audit_events_hash_chain'),
          ('audit_events', 'audit_events_reject_truncate'),
          ('audit_event_catalog', 'audit_event_catalog_immutable'),
          ('audit_event_catalog', 'audit_event_catalog_reject_truncate')
      )
      SELECT count(*)::text,
             count(*) FILTER (
               WHERE trigger_oid IS NULL OR enabled NOT IN ('O', 'A')
             )::text
        FROM (
          SELECT expected.table_name,
                 expected.trigger_name,
                 trigger.oid AS trigger_oid,
                 trigger.tgenabled::text AS enabled
            FROM expected
            LEFT JOIN pg_catalog.pg_class AS relation
              ON relation.relname = expected.table_name
             AND relation.relnamespace = 'public'::pg_catalog.regnamespace
            LEFT JOIN pg_catalog.pg_trigger AS trigger
              ON trigger.tgrelid = relation.oid
             AND trigger.tgname = expected.trigger_name
             AND NOT trigger.tgisinternal
        ) AS checked;
    "
  )" || die "failed to inspect immutable audit trigger coverage"
  IFS=$'\t' read -r trigger_count trigger_missing <<<"${trigger_summary}"
  [[ "${trigger_count}" == "5" && "${trigger_missing}" == "0" ]] ||
    die "required immutable audit triggers are absent or disabled"
  trigger_rows="$(
    run_psql "${database}" 1 --command="
      WITH expected(table_name, trigger_name) AS (
        VALUES
          ('audit_events', 'audit_events_append_only'),
          ('audit_events', 'audit_events_hash_chain'),
          ('audit_events', 'audit_events_reject_truncate'),
          ('audit_event_catalog', 'audit_event_catalog_immutable'),
          ('audit_event_catalog', 'audit_event_catalog_reject_truncate')
      )
      SELECT expected.table_name,
             expected.trigger_name,
             trigger.tgenabled::text
        FROM expected
        JOIN pg_catalog.pg_class AS relation
          ON relation.relname = expected.table_name
         AND relation.relnamespace = 'public'::pg_catalog.regnamespace
        JOIN pg_catalog.pg_trigger AS trigger
          ON trigger.tgrelid = relation.oid
         AND trigger.tgname = expected.trigger_name
         AND NOT trigger.tgisinternal
       ORDER BY expected.table_name, expected.trigger_name;
    "
  )" || die "failed to read immutable audit trigger state"
  trigger_digest="$(sha256_text "${trigger_rows}")"

  organization_count="$(
    run_psql "${database}" 1 \
      --set=verify_organization_id="${VERIFY_ORGANIZATION_ID}" <<'SQL'
SELECT count(*)::text
  FROM public.organizations
 WHERE id = :'verify_organization_id'::uuid;
SQL
  )" || die "failed to read the verification organization count"
  [[ "${organization_count}" == "1" ]] ||
    die "verification organization is absent or duplicated"
  organization_row="$(
    run_psql "${database}" 1 \
      --set=verify_organization_id="${VERIFY_ORGANIZATION_ID}" <<'SQL'
SELECT row_to_json(organization)::text
  FROM (
    SELECT id, slug, display_name, created_at, updated_at
      FROM public.organizations
     WHERE id = :'verify_organization_id'::uuid
  ) AS organization;
SQL
  )" || die "failed to read the verification organization"
  organization_digest="$(sha256_text "${organization_row}")"

  audit_summary="$(
    run_psql "${database}" 1 \
      --set=verify_organization_id="${VERIFY_ORGANIZATION_ID}" <<'SQL'
WITH event_count AS (
  SELECT count(*) AS count,
         COALESCE(max(chain_sequence), 0) AS max_sequence
    FROM public.audit_events
   WHERE organization_id = :'verify_organization_id'::uuid
),
chain_head AS (
  SELECT encode(event_hash, 'hex') AS event_hash
    FROM public.audit_events
   WHERE organization_id = :'verify_organization_id'::uuid
   ORDER BY chain_sequence DESC
   LIMIT 1
)
SELECT event_count.count::text,
       event_count.max_sequence::text,
       COALESCE(chain_head.event_hash, '')
  FROM event_count
  LEFT JOIN chain_head ON true;
SQL
  )" || die "failed to read verification audit-chain evidence"
  IFS=$'\t' read -r audit_count audit_max_sequence audit_head \
    <<<"${audit_summary}"
  [[ "${audit_count}" =~ ^[0-9]+$ ]] && ((audit_count > 0)) ||
    die "verification organization has no audit evidence"
  [[ "${audit_max_sequence}" =~ ^[0-9]+$ ]] &&
    [[ "${audit_head}" =~ ^[0-9a-f]{64}$ ]] ||
    die "verification organization audit-chain head is invalid"

  printf '%s\n' \
    "database_attributes_sha256=${database_attributes_digest}" \
    "migration_count=${migration_count}" \
    "migration_max_version=${migration_max}" \
    "migration_ledger_sha256=${migration_digest}" \
    "audit_catalog_count=${catalog_count}" \
    "audit_catalog_sha256=${catalog_digest}" \
    "required_trigger_count=${trigger_count}" \
    "required_trigger_sha256=${trigger_digest}" \
    "verification_organization_sha256=${organization_digest}" \
    "verification_audit_event_count=${audit_count}" \
    "verification_audit_max_sequence=${audit_max_sequence}" \
    "verification_audit_head=${audit_head}"
}

assert_restore_target() {
  restore_target_is_safe \
    "${RESTORE_DATABASE}" "${SOURCE_DATABASE}" "${MAINTENANCE_DATABASE}" ||
    die "refusing CREATE/DROP for an unsafe restore database"
}

create_restore_database() {
  local existing actual_marker
  assert_restore_target
  existing="$(
    run_psql "${MAINTENANCE_DATABASE}" 0 \
      --set=restore_database="${RESTORE_DATABASE}" <<'SQL'
SELECT count(*)::text
  FROM pg_catalog.pg_database
 WHERE datname = :'restore_database';
SQL
  )"
  [[ "${existing}" == "0" ]] ||
    die "generated restore database already exists"

  run_psql "${MAINTENANCE_DATABASE}" 0 \
    --set=restore_database="${RESTORE_DATABASE}" <<'SQL'
SELECT pg_catalog.format(
  'CREATE DATABASE %I WITH TEMPLATE template0',
  :'restore_database'
)
\gexec
SQL
  RESTORE_CREATED=1

  run_psql "${MAINTENANCE_DATABASE}" 0 \
    --set=restore_database="${RESTORE_DATABASE}" \
    --set=restore_marker="${RESTORE_MARKER}" <<'SQL'
SELECT pg_catalog.format(
  'COMMENT ON DATABASE %I IS %L',
  :'restore_database',
  :'restore_marker'
)
\gexec
SQL

  actual_marker="$(
    run_psql "${MAINTENANCE_DATABASE}" 0 \
      --set=restore_database="${RESTORE_DATABASE}" <<'SQL'
SELECT COALESCE(pg_catalog.shobj_description(oid, 'pg_database'), '')
  FROM pg_catalog.pg_database
 WHERE datname = :'restore_database';
SQL
  )"
  [[ "${actual_marker}" == "${RESTORE_MARKER}" ]] ||
    die "restore database marker was not committed"
}

drop_restore_database() {
  local marker_count remaining
  assert_restore_target
  marker_count="$(
    run_psql "${MAINTENANCE_DATABASE}" 0 \
      --set=restore_database="${RESTORE_DATABASE}" \
      --set=restore_marker="${RESTORE_MARKER}" <<'SQL'
SELECT count(*)::text
  FROM pg_catalog.pg_database
 WHERE datname = :'restore_database'
   AND pg_catalog.shobj_description(oid, 'pg_database') = :'restore_marker';
SQL
  )"
  if [[ "${marker_count}" == "0" ]]; then
    remaining="$(
      run_psql "${MAINTENANCE_DATABASE}" 0 \
        --set=restore_database="${RESTORE_DATABASE}" <<'SQL'
SELECT count(*)::text
  FROM pg_catalog.pg_database
 WHERE datname = :'restore_database';
SQL
    )"
    [[ "${remaining}" == "0" ]] && return 0
    info "refusing to drop restore database because its marker does not match"
    return 1
  fi
  [[ "${marker_count}" == "1" ]] || return 1

  run_psql "${MAINTENANCE_DATABASE}" 0 \
    --set=restore_database="${RESTORE_DATABASE}" \
    --set=restore_marker="${RESTORE_MARKER}" <<'SQL'
SELECT pg_catalog.format('DROP DATABASE %I WITH (FORCE)', :'restore_database')
  FROM pg_catalog.pg_database
 WHERE datname = :'restore_database'
   AND pg_catalog.shobj_description(oid, 'pg_database') = :'restore_marker'
\gexec
SQL
  remaining="$(
    run_psql "${MAINTENANCE_DATABASE}" 0 \
      --set=restore_database="${RESTORE_DATABASE}" <<'SQL'
SELECT count(*)::text
  FROM pg_catalog.pg_database
 WHERE datname = :'restore_database';
SQL
  )"
  [[ "${remaining}" == "0" ]]
}

cleanup() {
  local status=$?
  trap - EXIT
  set +e

  if ((RESTORE_CREATED)); then
    if drop_restore_database; then
      info "removed the exact generated restore database"
    else
      info \
        "cleanup failed closed for ${RESTORE_DATABASE}; DBA review is required"
      status=1
    fi
  fi

  if ((!PUBLICATION_COMPLETE)) &&
     [[ -n "${PARTIAL_ARCHIVE}" && -e "${PARTIAL_ARCHIVE}" &&
        -e "${ARCHIVE_PATH}" &&
        "${ARCHIVE_PATH}" -ef "${PARTIAL_ARCHIVE}" &&
        -n "${PARTIAL_EVIDENCE}" && -e "${PARTIAL_EVIDENCE}" &&
        -e "${ARCHIVE_PATH}.evidence" &&
        "${ARCHIVE_PATH}.evidence" -ef "${PARTIAL_EVIDENCE}" ]]; then
    PUBLICATION_COMPLETE=1
  fi

  if ((!PUBLICATION_COMPLETE)); then
    if [[ -n "${PARTIAL_ARCHIVE}" &&
          -e "${PARTIAL_ARCHIVE}" &&
          -e "${ARCHIVE_PATH}" &&
          "${ARCHIVE_PATH}" -ef "${PARTIAL_ARCHIVE}" ]]; then
      rm -f -- "${ARCHIVE_PATH}"
    elif ((FINAL_ARCHIVE_CREATED)); then
      info "refusing to remove an unverified final archive path"
      status=1
    fi
    if [[ -n "${PARTIAL_EVIDENCE}" &&
          -e "${PARTIAL_EVIDENCE}" &&
          -e "${ARCHIVE_PATH}.evidence" &&
          "${ARCHIVE_PATH}.evidence" -ef "${PARTIAL_EVIDENCE}" ]]; then
      rm -f -- "${ARCHIVE_PATH}.evidence"
    elif ((FINAL_EVIDENCE_CREATED)); then
      info "refusing to remove an unverified final evidence path"
      status=1
    fi
  fi

  [[ -z "${PARTIAL_ARCHIVE}" || ! -e "${PARTIAL_ARCHIVE}" ]] ||
    rm -f -- "${PARTIAL_ARCHIVE}"
  [[ -z "${PARTIAL_EVIDENCE}" || ! -e "${PARTIAL_EVIDENCE}" ]] ||
    rm -f -- "${PARTIAL_EVIDENCE}"

  exit "${status}"
}
trap cleanup EXIT
trap 'exit 130' INT
trap 'exit 143' TERM

actual_source="$(
  run_psql "${SOURCE_DATABASE}" 1 \
    --command="SELECT pg_catalog.current_database();"
)"
[[ "${actual_source}" == "${SOURCE_DATABASE}" ]] ||
  die "libpq connected to an unexpected source database"
source_read_only="$(
  run_psql "${SOURCE_DATABASE}" 1 --command="SHOW transaction_read_only;"
)"
[[ "${source_read_only}" == "on" ]] ||
  die "source verification session is not read-only"
actual_maintenance="$(
  run_psql "${MAINTENANCE_DATABASE}" 0 \
    --command="SELECT pg_catalog.current_database();"
)"
[[ "${actual_maintenance}" == "${MAINTENANCE_DATABASE}" ]] ||
  die "libpq connected to an unexpected maintenance database"

server_version_num="$(
  run_psql "${SOURCE_DATABASE}" 1 \
    --command="SELECT pg_catalog.current_setting('server_version_num');"
)"
[[ "${server_version_num}" =~ ^[0-9]+$ ]] ||
  die "PostgreSQL server version is unreadable"
server_major=$((server_version_num / 10000))
dump_version="$(
  LC_ALL=C pg_dump --version |
    awk '{
      for (field = 1; field <= NF; field++) {
        if ($field ~ /^[0-9]+\.[0-9]+/) {
          gsub(/[()]/, "", $field)
          print $field
          exit
        }
      }
    }'
)"
restore_version="$(
  LC_ALL=C pg_restore --version |
    awk '{
      for (field = 1; field <= NF; field++) {
        if ($field ~ /^[0-9]+\.[0-9]+/) {
          gsub(/[()]/, "", $field)
          print $field
          exit
        }
      }
    }'
)"
dump_major="${dump_version%%.*}"
restore_major="${restore_version%%.*}"
[[ "${dump_major}" =~ ^[0-9]+$ && "${restore_major}" =~ ^[0-9]+$ ]] ||
  die "PostgreSQL client versions are unreadable"
[[ "${dump_major}" == "${server_major}" ]] ||
  die "pg_dump major version differs from the PostgreSQL server"
[[ "${restore_major}" == "${server_major}" ]] ||
  die "pg_restore major version differs from the PostgreSQL server"

source_database_creation_attributes="$(
  collect_database_creation_attributes "${SOURCE_DATABASE}" "${SOURCE_DATABASE}"
)"
template0_database_creation_attributes="$(
  collect_database_creation_attributes "${MAINTENANCE_DATABASE}" "template0"
)"
[[ -n "${source_database_creation_attributes}" &&
   -n "${template0_database_creation_attributes}" ]] ||
  die "source or template0 database attributes are unreadable"
[[ "${source_database_creation_attributes}" == "${template0_database_creation_attributes}" ]] ||
  die "source encoding or locale attributes differ from template0"

info "capturing read-only source evidence"
source_evidence_before="$(collect_evidence "${SOURCE_DATABASE}")"

archive_directory="$(dirname -- "${ARCHIVE_PATH}")"
PARTIAL_ARCHIVE="$(
  mktemp "${archive_directory}/.copilotmail-backup.XXXXXX"
)"
PGDATABASE="${SOURCE_DATABASE}" \
PGOPTIONS="$(readonly_source_pgoptions)" \
  pg_dump \
    --format=custom \
    --no-owner \
    --no-acl \
    --no-password \
    --serializable-deferrable \
    --file="${PARTIAL_ARCHIVE}"
chmod 0600 "${PARTIAL_ARCHIVE}"
pg_restore --no-password --list "${PARTIAL_ARCHIVE}" >/dev/null

source_evidence_after="$(collect_evidence "${SOURCE_DATABASE}")"
[[ "${source_evidence_before}" == "${source_evidence_after}" ]] ||
  die "sampled source evidence changed while the backup was running"

archive_sha256="$(sha256sum "${PARTIAL_ARCHIVE}" | awk '{print $1}')"
[[ "${archive_sha256}" =~ ^[0-9a-f]{64}$ ]] ||
  die "backup archive digest is invalid"

info "creating an isolated generated restore database"
create_restore_database
PGDATABASE="${RESTORE_DATABASE}" PGOPTIONS="$(base_pgoptions)" \
  pg_restore \
    --dbname="${RESTORE_DATABASE}" \
    --exit-on-error \
    --single-transaction \
    --no-owner \
    --no-acl \
    --no-password \
    --no-tablespaces \
    "${PARTIAL_ARCHIVE}"

restore_read_only="$(
  run_psql "${RESTORE_DATABASE}" 1 --command="SHOW transaction_read_only;"
)"
[[ "${restore_read_only}" == "on" ]] ||
  die "restored database verification session is not read-only"
restored_evidence="$(collect_evidence "${RESTORE_DATABASE}")"
[[ "${source_evidence_before}" == "${restored_evidence}" ]] ||
  die "restored migration, audit, trigger, or seed evidence differs from source"

PARTIAL_EVIDENCE="$(
  mktemp "${archive_directory}/.copilotmail-evidence.XXXXXX"
)"
{
  printf '%s\n' \
    "format_version=1" \
    "archive_format=postgresql_custom" \
    "source_database=${SOURCE_DATABASE}" \
    "verification_organization_id=${VERIFY_ORGANIZATION_ID}" \
    "archive_sha256=${archive_sha256}" \
    "postgres_server_version_num=${server_version_num}" \
    "pg_dump_version=${dump_version}" \
    "verified_at=$(date -u +%Y-%m-%dT%H:%M:%SZ)"
  printf '%s\n' "${source_evidence_before}"
} >"${PARTIAL_EVIDENCE}"
chmod 0600 "${PARTIAL_EVIDENCE}"

info "removing the exact generated restore database"
if ! drop_restore_database; then
  die "restore database cleanup failed closed; DBA review is required"
fi
RESTORE_CREATED=0

archive_destination_is_safe "${ARCHIVE_PATH}" ||
  die "archive destination changed before no-clobber publication"
ln -- "${PARTIAL_EVIDENCE}" "${ARCHIVE_PATH}.evidence"
FINAL_EVIDENCE_CREATED=1
ln -- "${PARTIAL_ARCHIVE}" "${ARCHIVE_PATH}"
FINAL_ARCHIVE_CREATED=1
PUBLICATION_COMPLETE=1
if rm -f -- "${PARTIAL_ARCHIVE}"; then
  PARTIAL_ARCHIVE=""
else
  info "final archive is committed; hidden staging link cleanup will retry"
fi
if rm -f -- "${PARTIAL_EVIDENCE}"; then
  PARTIAL_EVIDENCE=""
else
  info "final evidence is committed; hidden staging link cleanup will retry"
fi

printf '%s\n' \
  "PostgreSQL logical backup and isolated restore verification passed." \
  "Archive: ${ARCHIVE_PATH}" \
  "Evidence: ${ARCHIVE_PATH}.evidence"
