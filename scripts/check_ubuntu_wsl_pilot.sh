#!/usr/bin/env bash
# Read-only, fail-closed preflight for the Ubuntu/WSL CopilotMail V2 Pilot.
#
# This script never sources an environment file, reads secret contents, starts
# containers, pulls images, changes permissions, or writes files. It inspects
# only allowlisted non-secret environment values and secret file metadata.

set -Eeuo pipefail

readonly CONTROL_UID=10001
readonly MIN_ENGINE_MAJOR=20
readonly MIN_ENGINE_MINOR=10
readonly MIN_COMPOSE_MAJOR=2
readonly MIN_COMPOSE_MINOR=20
readonly MIN_FREE_GIB=40
readonly MIN_FREE_INODES=200000

SCRIPT_DIR="$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")" && pwd -P)"
REPO_ROOT="$(cd -- "${SCRIPT_DIR}/.." && pwd -P)"
ENV_FILE="${REPO_ROOT}/deploy/.env.v2"
ACCESS_MODE=""
WITH_HUMAN_SESSION=0
WITH_MAIL=0
STALWART_BOOTSTRAP=0
WITH_AUDIT_MAINTENANCE=0
WITH_OBSERVABILITY=0
SELF_TEST=0
BLOCKERS=0
WARNINGS=0
DOCKER_SECURITY_OPTIONS=""

pass() {
  printf 'PASS: %s\n' "$*"
}

info() {
  printf 'INFO: %s\n' "$*"
}

warn() {
  printf 'WARN: %s\n' "$*" >&2
  ((WARNINGS += 1))
}

blocker() {
  printf 'BLOCKER: %s\n' "$*" >&2
  ((BLOCKERS += 1))
}

usage() {
  cat <<'EOF'
Usage:
  bash scripts/check_ubuntu_wsl_pilot.sh --access ssh-tunnel|https [options]
  bash scripts/check_ubuntu_wsl_pilot.sh --self-test

Options:
  --env-file PATH            Pilot env file (default: deploy/.env.v2).
  --with-human-session       Require Human OIDC BFF configuration and key.
  --stalwart-bootstrap       Validate the safe, Stalwart-only bootstrap stage.
  --with-mail                Require the complete Mail overlay and six secrets.
  --with-audit-maintenance   Require audit-maintenance keys and volume checks.
  --with-observability       Include observability ports and Compose overlay.
  --access MODE              Required remote-access boundary: ssh-tunnel|https.
  --self-test                Test pure rejection logic without Docker or files.
  -h, --help                 Show this help.

This command is read-only. It intentionally does not run `docker compose up`.
EOF
}

version_at_least() {
  local raw="$1"
  local required_major="$2"
  local required_minor="$3"
  local major minor
  raw="${raw#v}"
  if [[ ! "${raw}" =~ ^([0-9]+)\.([0-9]+) ]]; then
    return 1
  fi
  major="${BASH_REMATCH[1]}"
  minor="${BASH_REMATCH[2]}"
  ((major > required_major || (major == required_major && minor >= required_minor)))
}

supported_host_arch() {
  case "$1" in
    x86_64 | aarch64) return 0 ;;
    *) return 1 ;;
  esac
}

architectures_compatible() {
  case "$1:$2" in
    x86_64:amd64 | x86_64:x86_64 | aarch64:arm64 | aarch64:aarch64) return 0 ;;
    *) return 1 ;;
  esac
}

path_is_windows_mount() {
  [[ "$1" == /mnt/* ]]
}

mount_options_are_writable() {
  local options=",${1},"
  [[ "${options}" == *,rw,* ]] && [[ "${options}" != *,ro,* ]]
}

compose_control_environment_is_clean() {
  local key
  for key in \
    COMPOSE_PROJECT_NAME \
    COMPOSE_PROFILES \
    COMPOSE_FILE \
    COMPOSE_ENV_FILES; do
    if [[ -v "${key}" ]]; then
      return 1
    fi
  done
}

interpolation_environment_is_clean() {
  local key
  for key in "$@"; do
    if [[ -v "${key}" ]]; then
      return 1
    fi
  done
}

extract_compose_interpolation_variables() {
  awk '
    {
      line = $0
      while (match(line, /\$\{[A-Za-z_][A-Za-z0-9_]*/)) {
        if (RSTART == 1 || substr(line, RSTART - 1, 1) != "$") {
          print substr(line, RSTART + 2, RLENGTH - 2)
        }
        line = substr(line, RSTART + RLENGTH)
      }
    }
  ' "$@" | LC_ALL=C sort -u
}

validate_compose_config_json() {
  python3 -c '
import json
import sys

try:
    config = json.load(sys.stdin)
except (json.JSONDecodeError, UnicodeDecodeError):
    raise SystemExit(1)

if not isinstance(config, dict) or config.get("name") != "copilotmail-v2-pilot":
    raise SystemExit(1)

services = config.get("services")
if not isinstance(services, dict) or not services:
    raise SystemExit(1)

for service in services.values():
    if not isinstance(service, dict):
        raise SystemExit(1)
    ports = service.get("ports") or []
    if not isinstance(ports, list):
        raise SystemExit(1)
    for port in ports:
        if not isinstance(port, dict):
            raise SystemExit(1)
        published = port.get("published")
        if published not in (None, "") and port.get("host_ip") != "127.0.0.1":
            raise SystemExit(1)
'
}

compose_extraction_self_test() {
  local actual
  actual="$(
    printf '%s\n' \
      'image: ${IMAGE_NAME:-example.test/image}:$${LITERAL_TAG}' \
      'value: ${REQUIRED_VALUE:?required}' \
      'other: ${IMAGE_NAME}' |
      extract_compose_interpolation_variables
  )"
  [[ "${actual}" == $'IMAGE_NAME\nREQUIRED_VALUE' ]]
}

self_test_clean_compose_controls() {
  (
    unset COMPOSE_PROJECT_NAME COMPOSE_PROFILES COMPOSE_FILE COMPOSE_ENV_FILES
    compose_control_environment_is_clean
  )
}

self_test_reject_compose_control() {
  (
    unset COMPOSE_PROJECT_NAME COMPOSE_PROFILES COMPOSE_FILE COMPOSE_ENV_FILES
    COMPOSE_FILE=unexpected.yaml
    export COMPOSE_FILE
    compose_control_environment_is_clean
  )
}

self_test_clean_interpolation_environment() {
  (
    unset COPILOTMAIL_SELF_TEST_IMAGE
    interpolation_environment_is_clean COPILOTMAIL_SELF_TEST_IMAGE
  )
}

self_test_reject_interpolation_environment() {
  (
    COPILOTMAIL_SELF_TEST_IMAGE=""
    export COPILOTMAIL_SELF_TEST_IMAGE
    interpolation_environment_is_clean COPILOTMAIL_SELF_TEST_IMAGE
  )
}

self_test_valid_compose_config_json() {
  printf '%s' \
    '{"name":"copilotmail-v2-pilot","services":{"api":{"ports":[{"published":"8081","host_ip":"127.0.0.1"}]}}}' |
    validate_compose_config_json
}

self_test_reject_wrong_project_name() {
  printf '%s' \
    '{"name":"unexpected","services":{"api":{"ports":[{"published":"8081","host_ip":"127.0.0.1"}]}}}' |
    validate_compose_config_json
}

self_test_reject_wildcard_published_port() {
  printf '%s' \
    '{"name":"copilotmail-v2-pilot","services":{"api":{"ports":[{"published":"8081","host_ip":"0.0.0.0"}]}}}' |
    validate_compose_config_json
}

unsigned_integer() {
  [[ "$1" =~ ^[0-9]+$ ]]
}

port_number() {
  unsigned_integer "$1" && ((1 <= 10#$1 && 10#$1 <= 65535))
}

private_file_mode() {
  local mode="$1"
  if [[ ! "${mode}" =~ ^[0-7]{3,4}$ ]]; then
    return 1
  fi
  local permissions=$((8#${mode}))
  (( (permissions & 0077) == 0 && (permissions & 0111) == 0 ))
}

secret_mode_readable_by_control_uid() {
  local owner_uid="$1"
  local mode="$2"
  if [[ "${owner_uid}" != "${CONTROL_UID}" ]] || ! private_file_mode "${mode}"; then
    return 1
  fi
  local permissions=$((8#${mode}))
  (( (permissions & 0400) != 0 ))
}

secret_size_valid() {
  local size="$1"
  local minimum="$2"
  local maximum="$3"
  local exact="${4:-0}"
  unsigned_integer "${size}" || return 1
  if ((exact > 0)); then
    ((size == exact))
  else
    ((minimum <= size && size <= maximum))
  fi
}

loopback_bind_value() {
  [[ "$1" == "127.0.0.1" ]]
}

loopback_host_port() {
  local value="$1"
  [[ "${value}" =~ ^127\.0\.0\.1:([0-9]+)$ ]] &&
    port_number "${BASH_REMATCH[1]}"
}

https_origin() {
  local value="$1"
  if [[ ! "${value}" =~ ^https://[A-Za-z0-9.-]+(:([0-9]+))?$ ]]; then
    return 1
  fi
  [[ -z "${BASH_REMATCH[2]:-}" ]] || port_number "${BASH_REMATCH[2]}"
}

loopback_http_origin() {
  local value="$1"
  local host port first second third fourth
  if [[ ! "${value}" =~ ^http://(localhost|127\.[0-9]{1,3}\.[0-9]{1,3}\.[0-9]{1,3})(:([0-9]+))?$ ]]; then
    return 1
  fi
  host="${BASH_REMATCH[1]}"
  port="${BASH_REMATCH[3]:-}"
  if [[ "${host}" == 127.* ]]; then
    IFS='.' read -r first second third fourth <<<"${host}"
    if ((10#${first} != 127 ||
      10#${second} > 255 ||
      10#${third} > 255 ||
      10#${fourth} > 255)); then
      return 1
    fi
  fi
  [[ -z "${port}" ]] || port_number "${port}"
}

uuid_value() {
  [[ "$1" =~ ^[0-9a-fA-F]{8}-[0-9a-fA-F]{4}-[1-8][0-9a-fA-F]{3}-[89abAB][0-9a-fA-F]{3}-[0-9a-fA-F]{12}$ ]]
}

expect_success() {
  local label="$1"
  shift
  if "$@"; then
    pass "self-test accepted ${label}"
  else
    printf 'SELF-TEST FAILED: expected success for %s\n' "${label}" >&2
    return 1
  fi
}

expect_failure() {
  local label="$1"
  shift
  if "$@"; then
    printf 'SELF-TEST FAILED: expected rejection for %s\n' "${label}" >&2
    return 1
  else
    pass "self-test rejected ${label}"
  fi
}

run_self_test() {
  local failed=0
  expect_success "supported Docker version" version_at_least "24.0.7" 20 10 || failed=1
  expect_failure "old Docker version" version_at_least "19.03.15" 20 10 || failed=1
  expect_success "supported Compose version" version_at_least "v2.24.6" 2 20 || failed=1
  expect_failure "Compose v1" version_at_least "1.29.2" 2 20 || failed=1
  expect_success "amd64 architecture" architectures_compatible "x86_64" "amd64" || failed=1
  expect_failure "cross architecture" architectures_compatible "x86_64" "arm64" || failed=1
  expect_success "Windows mount detection" path_is_windows_mount "/mnt/c/src/CopilotMail" || failed=1
  expect_failure "Linux filesystem path" path_is_windows_mount "/home/pilot/CopilotMail" || failed=1
  expect_success "writable mount options" mount_options_are_writable "rw,relatime" || failed=1
  expect_failure "read-only mount options" mount_options_are_writable "ro,relatime" || failed=1
  expect_failure "ambiguous mount options" mount_options_are_writable "rw,ro,relatime" || failed=1
  expect_success "unset Compose control environment" self_test_clean_compose_controls || failed=1
  expect_failure "ambient COMPOSE_FILE" self_test_reject_compose_control || failed=1
  expect_success "Compose interpolation extraction" compose_extraction_self_test || failed=1
  expect_success "unset interpolation environment" self_test_clean_interpolation_environment || failed=1
  expect_failure "empty ambient interpolation override" self_test_reject_interpolation_environment || failed=1
  expect_success "loopback-only rendered Compose JSON" self_test_valid_compose_config_json || failed=1
  expect_failure "wrong rendered Compose project name" self_test_reject_wrong_project_name || failed=1
  expect_failure "wildcard rendered published port" self_test_reject_wildcard_published_port || failed=1
  expect_success "private readable secret" secret_mode_readable_by_control_uid "10001" "400" || failed=1
  expect_failure "wrong secret owner" secret_mode_readable_by_control_uid "1000" "400" || failed=1
  expect_failure "world-readable secret" secret_mode_readable_by_control_uid "10001" "404" || failed=1
  expect_failure "executable secret" secret_mode_readable_by_control_uid "10001" "500" || failed=1
  expect_success "exact Human key size" secret_size_valid "32" "32" "32" "32" || failed=1
  expect_failure "Base64-sized Human key" secret_size_valid "44" "32" "32" "32" || failed=1
  expect_success "valid TCP port" port_number "8081" || failed=1
  expect_failure "out-of-range TCP port" port_number "70000" || failed=1
  expect_success "loopback bind" loopback_bind_value "127.0.0.1" || failed=1
  expect_failure "wildcard bind" loopback_bind_value "0.0.0.0" || failed=1
  expect_success "HTTPS origin" https_origin "https://admin.example.test" || failed=1
  expect_failure "HTTPS origin with path" https_origin "https://admin.example.test/app" || failed=1
  expect_failure "HTTPS origin with invalid port" https_origin "https://admin.example.test:70000" || failed=1
  expect_success "loopback development origin" loopback_http_origin "http://127.0.0.1:8083" || failed=1
  expect_failure "private-LAN HTTP origin" loopback_http_origin "http://192.168.3.27:8083" || failed=1
  expect_failure "invalid loopback address" loopback_http_origin "http://127.0.999.1:8083" || failed=1
  expect_success "UUIDv7 resource ID" uuid_value "019c0000-0000-7000-8000-000000000001" || failed=1
  expect_failure "zero placeholder UUID" uuid_value "00000000-0000-0000-0000-000000000000" || failed=1
  if ((failed != 0)); then
    return 1
  fi
  printf 'Ubuntu/WSL Pilot preflight self-test passed without Docker.\n'
}

while (($# > 0)); do
  case "$1" in
    --env-file)
      if (($# < 2)); then
        printf '%s\n' "--env-file requires a path" >&2
        exit 2
      fi
      ENV_FILE="$2"
      shift 2
      ;;
    --access)
      if (($# < 2)); then
        printf '%s\n' "--access requires ssh-tunnel or https" >&2
        exit 2
      fi
      ACCESS_MODE="$2"
      shift 2
      ;;
    --with-human-session)
      WITH_HUMAN_SESSION=1
      shift
      ;;
    --stalwart-bootstrap)
      STALWART_BOOTSTRAP=1
      shift
      ;;
    --with-mail)
      WITH_MAIL=1
      shift
      ;;
    --with-audit-maintenance)
      WITH_AUDIT_MAINTENANCE=1
      shift
      ;;
    --with-observability)
      WITH_OBSERVABILITY=1
      shift
      ;;
    --self-test)
      SELF_TEST=1
      shift
      ;;
    -h | --help)
      usage
      exit 0
      ;;
    *)
      printf 'unknown option: %s\n' "$1" >&2
      usage >&2
      exit 2
      ;;
  esac
done

if ((SELF_TEST)); then
  run_self_test
  exit $?
fi

printf '%s\n' \
  'POLICY: 禁止裸执行 `docker compose up`；仓库根目录 Compose 是旧 V1。' \
  'POLICY: 只允许显式传入 --env-file，把 compose.dev.yaml 放在第一个 -f，并把 compose.pilot.yaml 放在最后一个 -f。' \
  'POLICY: 本检查只支持隔离 Ubuntu/WSL Pilot，不构成生产放行。'

case "${ACCESS_MODE}" in
  ssh-tunnel | https)
    pass "remote access mode is explicit: ${ACCESS_MODE}"
    ;;
  "")
    blocker "--access ssh-tunnel|https is required"
    ;;
  *)
    blocker "unsupported access mode; use ssh-tunnel or https"
    ;;
esac

if ((STALWART_BOOTSTRAP && WITH_MAIL)); then
  blocker "--stalwart-bootstrap and --with-mail are separate stages and cannot be combined"
fi

current_directory="$(pwd -P)"
if [[ "${current_directory}" == "${REPO_ROOT}" ]]; then
  pass "preflight is running from the repository root"
else
  blocker "run the preflight and every documented Compose command from ${REPO_ROOT}"
fi

if path_is_windows_mount "${REPO_ROOT}"; then
  blocker "repository is under /mnt; clone it into the WSL Linux filesystem"
else
  pass "repository is outside /mnt"
fi

repo_fs="$(stat -f -c '%T' "${REPO_ROOT}" 2>/dev/null || true)"
case "${repo_fs}" in
  9p | v9fs | drvfs | cifs | fuseblk | ntfs*)
    blocker "repository filesystem ${repo_fs} is not accepted for the Pilot"
    ;;
  "")
    blocker "repository filesystem type could not be determined"
    ;;
  *)
    pass "repository filesystem type is ${repo_fs}"
    ;;
esac

required_deployment_files=(
  deploy/compose.dev.yaml
  deploy/compose.pilot.yaml
  deploy/docker/Dockerfile.control-plane
)
selected_compose_files=(
  "${REPO_ROOT}/deploy/compose.dev.yaml"
)
if ((WITH_HUMAN_SESSION)); then
  required_deployment_files+=(deploy/compose.human-session.yaml)
  selected_compose_files+=("${REPO_ROOT}/deploy/compose.human-session.yaml")
fi
if ((WITH_MAIL || STALWART_BOOTSTRAP)); then
  required_deployment_files+=(deploy/compose.mail-poc.yaml)
  selected_compose_files+=("${REPO_ROOT}/deploy/compose.mail-poc.yaml")
fi
if ((STALWART_BOOTSTRAP)); then
  required_deployment_files+=(deploy/compose.mail-bootstrap.yaml)
  selected_compose_files+=("${REPO_ROOT}/deploy/compose.mail-bootstrap.yaml")
fi
if ((WITH_OBSERVABILITY)); then
  required_deployment_files+=(deploy/compose.observability.yaml)
  selected_compose_files+=("${REPO_ROOT}/deploy/compose.observability.yaml")
fi
selected_compose_files+=("${REPO_ROOT}/deploy/compose.pilot.yaml")
for relative_path in "${required_deployment_files[@]}"; do
  if [[ -f "${REPO_ROOT}/${relative_path}" ]]; then
    pass "required deployment file exists: ${relative_path}"
  else
    blocker "required deployment file is missing: ${relative_path}"
  fi
done

compose_control_keys=(
  COMPOSE_PROJECT_NAME
  COMPOSE_PROFILES
  COMPOSE_FILE
  COMPOSE_ENV_FILES
)
if compose_control_environment_is_clean; then
  pass "ambient Compose control environment is unset"
else
  for key in "${compose_control_keys[@]}"; do
    if [[ -v "${key}" ]]; then
      blocker "ambient ${key} is set; unset it so explicit Pilot arguments remain authoritative"
    fi
  done
fi

existing_compose_files=()
for compose_file in "${selected_compose_files[@]}"; do
  if [[ -f "${compose_file}" ]]; then
    existing_compose_files+=("${compose_file}")
  fi
done
compose_interpolation_vars=()
if ((${#existing_compose_files[@]} > 0)); then
  mapfile -t compose_interpolation_vars < <(
    extract_compose_interpolation_variables "${existing_compose_files[@]}"
  )
fi
if interpolation_environment_is_clean "${compose_interpolation_vars[@]}"; then
  pass "ambient shell does not override selected Compose interpolation variables"
else
  for key in "${compose_interpolation_vars[@]}"; do
    if [[ -v "${key}" ]]; then
      blocker "ambient ${key} is set; unset it and keep the Pilot value in the explicit env file"
    fi
  done
fi

if [[ "${ENV_FILE}" != /* ]]; then
  ENV_FILE="$(realpath -m -- "${REPO_ROOT}/${ENV_FILE}")"
fi
if [[ -L "${ENV_FILE}" ]]; then
  blocker "Pilot env file must not be a symbolic link"
elif [[ ! -f "${ENV_FILE}" ]]; then
  blocker "Pilot env file is missing; copy deploy/.env.v2.example outside Git first"
else
  env_mode="$(stat -c '%a' "${ENV_FILE}" 2>/dev/null || true)"
  if private_file_mode "${env_mode}"; then
    pass "Pilot env file metadata is private"
  else
    blocker "Pilot env file must be non-executable and inaccessible to group/other"
  fi
fi

for command_name in docker df stat findmnt ss awk grep uname ip hostname realpath timeout env python3 sort; do
  if command -v "${command_name}" >/dev/null 2>&1; then
    pass "required command is available: ${command_name}"
  else
    blocker "required command is unavailable: ${command_name}"
  fi
done

repo_mount_options="$(
  findmnt --noheadings --output OPTIONS --target "${REPO_ROOT}" 2>/dev/null ||
    true
)"
if mount_options_are_writable "${repo_mount_options}"; then
  pass "repository filesystem is mounted read-write"
elif [[ -n "${repo_mount_options}" ]]; then
  blocker "repository filesystem is not safely mounted read-write"
else
  blocker "repository mount options could not be determined"
fi

if grep -q -- '--uid 10001' "${REPO_ROOT}/deploy/docker/Dockerfile.control-plane" &&
  grep -q '^USER copilotmail$' "${REPO_ROOT}/deploy/docker/Dockerfile.control-plane"; then
  pass "control-plane image still declares runtime UID ${CONTROL_UID}"
else
  blocker "control-plane runtime UID drifted from the preflight ownership policy"
fi

host_arch="$(uname -m 2>/dev/null || true)"
if supported_host_arch "${host_arch}"; then
  pass "host architecture is supported: ${host_arch}"
else
  blocker "unsupported host architecture: ${host_arch:-unknown}"
fi

if grep -qi microsoft /proc/version 2>/dev/null ||
  [[ -n "${WSL_INTEROP:-}" ]]; then
  pass "WSL environment detected"
  if ip link show lo 2>/dev/null | grep -q 'UP'; then
    pass "WSL loopback interface is up"
  else
    blocker "WSL loopback interface is not up"
  fi
  if ip -4 route show default 2>/dev/null | grep -q '^default '; then
    pass "WSL IPv4 default route exists"
  else
    blocker "WSL IPv4 default route is missing"
  fi
  non_loopback_ipv4_count="$(
    hostname -I 2>/dev/null |
      awk '{
        count = 0
        for (field = 1; field <= NF; field++) {
          if ($field ~ /^[0-9]+\.[0-9]+\.[0-9]+\.[0-9]+$/ &&
              $field !~ /^127\./) {
            count++
          }
        }
        print count
      }'
  )"
  if unsigned_integer "${non_loopback_ipv4_count}" &&
    ((non_loopback_ipv4_count > 0)); then
    pass "WSL has at least one non-loopback IPv4 address"
  else
    blocker "WSL has no non-loopback IPv4 address"
  fi
  warn "WSL NAT/mirrored mode and Windows Firewall cannot be proven from Linux alone"
else
  warn "WSL was not detected; continuing as native Ubuntu"
fi

if [[ "${ACCESS_MODE}" == "ssh-tunnel" ]]; then
  if [[ -n "${SSH_CONNECTION:-}" ]] ||
    ss -H -ltn 2>/dev/null |
      awk '{local = $4; count = split(local, parts, ":"); if (parts[count] == "22") found = 1} END {exit !found}'; then
    pass "an SSH path exists for loopback port forwarding"
  else
    blocker "ssh-tunnel mode requires an active SSH session or TCP/22 listener"
  fi
fi

docker_ready=0
docker_info_record=""
if command -v docker >/dev/null 2>&1 &&
  docker_info_record="$(
    timeout 15s docker info \
      --format '{{.OSType}}|{{.Architecture}}|{{.DockerRootDir}}|{{json .SecurityOptions}}' \
      2>/dev/null
  )"; then
  docker_ready=1
  pass "Docker daemon is reachable without privilege escalation"
else
  blocker "Docker daemon is unavailable to the current user"
fi

docker_root=""
if ((docker_ready)); then
  IFS='|' read -r daemon_os daemon_arch docker_root DOCKER_SECURITY_OPTIONS \
    <<<"${docker_info_record}"
  engine_version="$(
    timeout 10s docker version --format '{{.Server.Version}}' 2>/dev/null || true
  )"
  if version_at_least "${engine_version}" "${MIN_ENGINE_MAJOR}" "${MIN_ENGINE_MINOR}"; then
    pass "Docker Engine version satisfies host-gateway requirement"
  else
    blocker "Docker Engine ${engine_version:-unknown} is older than 20.10"
  fi

  compose_version="$(
    timeout 10s docker compose version --short 2>/dev/null || true
  )"
  if version_at_least "${compose_version}" "${MIN_COMPOSE_MAJOR}" "${MIN_COMPOSE_MINOR}"; then
    pass "Docker Compose version is supported"
  else
    blocker "Docker Compose ${compose_version:-unknown} is older than 2.20"
  fi

  if [[ "${daemon_os}" == "linux" ]]; then
    pass "Docker daemon uses Linux containers"
  else
    blocker "Docker daemon is not using Linux containers"
  fi
  if architectures_compatible "${host_arch}" "${daemon_arch}"; then
    pass "host and Docker daemon architectures agree"
  else
    blocker "host ${host_arch:-unknown} and daemon ${daemon_arch:-unknown} architectures differ"
  fi

  if [[ "${DOCKER_SECURITY_OPTIONS}" == *rootless* ||
    "${DOCKER_SECURITY_OPTIONS}" == *userns* ]]; then
    warn "rootless/userns Docker detected; numeric bind-mount ownership needs a runtime proof"
  fi
fi

check_capacity() {
  local label="$1"
  local path="$2"
  local available_kib available_inodes
  if [[ ! -e "${path}" ]]; then
    blocker "${label} path is not visible: ${path}"
    return
  fi
  available_kib="$(
    df -Pk -- "${path}" 2>/dev/null |
      awk 'NR == 2 {print $4}'
  )"
  available_inodes="$(
    df -Pi -- "${path}" 2>/dev/null |
      awk 'NR == 2 {print $4}'
  )"
  if ! unsigned_integer "${available_kib}"; then
    blocker "${label} free disk could not be measured"
  elif ((available_kib < MIN_FREE_GIB * 1024 * 1024)); then
    blocker "${label} has less than ${MIN_FREE_GIB} GiB free"
  else
    pass "${label} has at least ${MIN_FREE_GIB} GiB free"
  fi
  if ! unsigned_integer "${available_inodes}"; then
    blocker "${label} free inodes could not be measured"
  elif ((available_inodes < MIN_FREE_INODES)); then
    blocker "${label} has fewer than ${MIN_FREE_INODES} free inodes"
  else
    pass "${label} has at least ${MIN_FREE_INODES} free inodes"
  fi
}

check_capacity "repository filesystem" "${REPO_ROOT}"
if ((docker_ready)); then
  if [[ -n "${docker_root}" ]]; then
    check_capacity "Docker data-root filesystem" "${docker_root}"
    docker_root_mount_options="$(
      findmnt --noheadings --output OPTIONS --target "${docker_root}" 2>/dev/null ||
        true
    )"
    if mount_options_are_writable "${docker_root_mount_options}"; then
      pass "Docker data-root filesystem is mounted read-write"
    elif [[ -n "${docker_root_mount_options}" ]]; then
      blocker "Docker data-root filesystem is not safely mounted read-write"
    else
      blocker "Docker data-root mount options could not be determined"
    fi
  else
    blocker "Docker data-root path could not be determined"
  fi
fi

lookup_env_value() {
  local key="$1"
  awk -v key="${key}" '
    /^[[:space:]]*#/ || /^[[:space:]]*$/ { next }
    index($0, key "=") == 1 {
      found++
      value = substr($0, length(key) + 2)
      sub(/\r$/, "", value)
    }
    END {
      if (found == 1) {
        print value
        exit 0
      }
      if (found > 1) {
        exit 2
      }
      exit 1
    }
  ' "${ENV_FILE}"
}

allowlisted_env_keys=(
  COPILOTMAIL_DEV_BIND_HOST
  COPILOTMAIL_V2_HTTP_PORT
  COPILOTMAIL_ADMIN_HTTP_PORT
  COPILOTMAIL_MAIL_APP_HTTP_PORT
  COPILOTMAIL_DEV_MINIO_API_PORT
  COPILOTMAIL_DEV_MINIO_CONSOLE_PORT
  COPILOTMAIL_DEV_STALWART_HTTP_PORT
  COPILOTMAIL_DEV_STALWART_SMTP_PORT
  COPILOTMAIL_DEV_STALWART_SUBMISSION_PORT
  COPILOTMAIL_DEV_STALWART_SUBMISSIONS_PORT
  COPILOTMAIL_DEV_STALWART_IMAP_PORT
  COPILOTMAIL_DEV_STALWART_IMAPS_PORT
  COPILOTMAIL_PROMETHEUS_BIND
  COPILOTMAIL_GRAFANA_BIND
  COPILOTMAIL_OIDC_ISSUER
  COPILOTMAIL_OIDC_AUDIENCE
  COPILOTMAIL_OIDC_ALLOW_INSECURE_HTTP
  COPILOTMAIL_ADMIN_OIDC_CLIENT_ID
  COPILOTMAIL_ADMIN_PUBLIC_ORIGIN
  COPILOTMAIL_ADMIN_ORGANIZATION_ID
  COPILOTMAIL_MAIL_APP_OIDC_CLIENT_ID
  COPILOTMAIL_MAIL_APP_PUBLIC_ORIGIN
  COPILOTMAIL_MAIL_APP_MAILBOX_ID
  COPILOTMAIL_HUMAN_SESSION_TTL_SECONDS
  COPILOTMAIL_HUMAN_SESSION_ALLOW_INSECURE_COOKIE
  COPILOTMAIL_STALWART_PUBLIC_URL
  COPILOTMAIL_MAIL_DATA_PLANE_ID
  COPILOTMAIL_MAIL_DATA_PLANE_ORIGIN
)
if [[ -f "${ENV_FILE}" ]]; then
  for key in "${allowlisted_env_keys[@]}"; do
    if lookup_env_value "${key}" >/dev/null 2>&1; then
      :
    else
      lookup_status=$?
      if ((lookup_status == 2)); then
        blocker "Pilot env file defines ${key} more than once"
      fi
    fi
  done
fi

env_value_or_default() {
  local key="$1"
  local default="$2"
  local value
  if [[ ! -f "${ENV_FILE}" ]]; then
    printf '%s' "${default}"
    return
  fi
  if value="$(lookup_env_value "${key}")"; then
    printf '%s' "${value}"
    return
  fi
  printf '%s' "${default}"
}

require_nonsecret_env() {
  local key="$1"
  local value status
  if [[ ! -f "${ENV_FILE}" ]]; then
    blocker "cannot validate ${key} because the Pilot env file is missing"
    return 1
  fi
  if value="$(lookup_env_value "${key}")"; then
    if [[ -n "${value}" ]]; then
      return 0
    fi
  else
    status=$?
    if ((status == 2)); then
      blocker "Pilot env file defines ${key} more than once"
      return 1
    fi
  fi
  blocker "required non-secret setting ${key} is missing or empty"
  return 1
}

bind_host="$(env_value_or_default COPILOTMAIL_DEV_BIND_HOST 127.0.0.1)"
if loopback_bind_value "${bind_host}"; then
  pass "shared development bind host remains 127.0.0.1"
else
  blocker "COPILOTMAIL_DEV_BIND_HOST must remain 127.0.0.1; use a tunnel or HTTPS proxy"
fi

declare -a port_specs=(
  "COPILOTMAIL_V2_HTTP_PORT:8081"
  "COPILOTMAIL_ADMIN_HTTP_PORT:8083"
  "COPILOTMAIL_MAIL_APP_HTTP_PORT:8084"
  "COPILOTMAIL_DEV_MINIO_API_PORT:9000"
  "COPILOTMAIL_DEV_MINIO_CONSOLE_PORT:9001"
)
if ((WITH_MAIL || STALWART_BOOTSTRAP)); then
  port_specs+=(
    "COPILOTMAIL_DEV_STALWART_HTTP_PORT:8082"
    "COPILOTMAIL_DEV_STALWART_SMTP_PORT:2525"
    "COPILOTMAIL_DEV_STALWART_SUBMISSION_PORT:2587"
    "COPILOTMAIL_DEV_STALWART_SUBMISSIONS_PORT:2465"
    "COPILOTMAIL_DEV_STALWART_IMAP_PORT:2143"
    "COPILOTMAIL_DEV_STALWART_IMAPS_PORT:2993"
  )
fi

declare -A seen_ports=()
for spec in "${port_specs[@]}"; do
  key="${spec%%:*}"
  default="${spec#*:}"
  port="$(env_value_or_default "${key}" "${default}")"
  if ! port_number "${port}"; then
    blocker "${key} is not a valid TCP port"
    continue
  fi
  if [[ -n "${seen_ports[${port}]:-}" ]]; then
    blocker "${key} duplicates host port ${port}"
    continue
  fi
  seen_ports["${port}"]="${key}"
  if ss -H -ltn 2>/dev/null |
    awk -v expected="${port}" '{
      local = $4
      count = split(local, parts, ":")
      if (parts[count] == expected) {
        found = 1
      }
    } END {exit !found}'; then
    blocker "host TCP port ${port} required by ${key} is already in use"
  else
    pass "host TCP port ${port} is available"
  fi
done

if ((WITH_OBSERVABILITY)); then
  prometheus_bind="$(env_value_or_default COPILOTMAIL_PROMETHEUS_BIND 127.0.0.1:9091)"
  grafana_bind="$(env_value_or_default COPILOTMAIL_GRAFANA_BIND 127.0.0.1:3000)"
  for entry in \
    "COPILOTMAIL_PROMETHEUS_BIND:${prometheus_bind}" \
    "COPILOTMAIL_GRAFANA_BIND:${grafana_bind}"; do
    key="${entry%%:*}"
    value="${entry#*:}"
    if loopback_host_port "${value}"; then
      port="${value##*:}"
      if [[ -n "${seen_ports[${port}]:-}" ]]; then
        blocker "${key} duplicates host port ${port}"
      elif ss -H -ltn 2>/dev/null |
        awk -v expected="${port}" '{
          local = $4
          count = split(local, parts, ":")
          if (parts[count] == expected) {
            found = 1
          }
        } END {exit !found}'; then
        blocker "host TCP port ${port} required by ${key} is already in use"
      else
        seen_ports["${port}"]="${key}"
        pass "${key} is loopback-bound and available"
      fi
    else
      blocker "${key} must use an explicit 127.0.0.1:PORT value"
    fi
  done
fi

if ((WITH_HUMAN_SESSION)); then
  human_keys=(
    COPILOTMAIL_OIDC_ISSUER
    COPILOTMAIL_OIDC_AUDIENCE
    COPILOTMAIL_ADMIN_OIDC_CLIENT_ID
    COPILOTMAIL_ADMIN_PUBLIC_ORIGIN
    COPILOTMAIL_ADMIN_ORGANIZATION_ID
    COPILOTMAIL_MAIL_APP_OIDC_CLIENT_ID
    COPILOTMAIL_MAIL_APP_PUBLIC_ORIGIN
  )
  for key in "${human_keys[@]}"; do
    require_nonsecret_env "${key}" || true
  done
  admin_origin="$(env_value_or_default COPILOTMAIL_ADMIN_PUBLIC_ORIGIN "")"
  mail_origin="$(env_value_or_default COPILOTMAIL_MAIL_APP_PUBLIC_ORIGIN "")"
  oidc_issuer="$(env_value_or_default COPILOTMAIL_OIDC_ISSUER "")"
  oidc_allow_insecure="$(env_value_or_default COPILOTMAIL_OIDC_ALLOW_INSECURE_HTTP false)"
  insecure_cookie="$(env_value_or_default COPILOTMAIL_HUMAN_SESSION_ALLOW_INSECURE_COOKIE false)"
  if [[ "${ACCESS_MODE}" == "https" ]]; then
    https_origin "${admin_origin}" ||
      blocker "Admin public origin must be an origin-only HTTPS URL"
    https_origin "${mail_origin}" ||
      blocker "Mail public origin must be an origin-only HTTPS URL"
    [[ "${oidc_issuer}" == https://* ]] ||
      blocker "HTTPS Pilot requires an HTTPS OIDC issuer"
    if [[ "${oidc_allow_insecure}" != "false" ||
      "${insecure_cookie}" != "false" ]]; then
      blocker "HTTPS Pilot must disable both insecure OIDC and insecure-cookie escape hatches"
    fi
  elif [[ "${ACCESS_MODE}" == "ssh-tunnel" ]]; then
    loopback_http_origin "${admin_origin}" ||
      blocker "SSH-tunnel Admin origin must use loopback HTTP"
    loopback_http_origin "${mail_origin}" ||
      blocker "SSH-tunnel Mail origin must use loopback HTTP"
    if [[ "${insecure_cookie}" != "true" ]]; then
      blocker "SSH-tunnel Human Session requires the development-only insecure-cookie flag"
    fi
    if [[ "${oidc_issuer}" == http://* &&
      "${oidc_allow_insecure}" != "true" ]]; then
      blocker "an HTTP loopback OIDC issuer requires its development-only insecure flag"
    fi
  fi
  if [[ -n "${admin_origin}" && "${admin_origin}" == "${mail_origin}" ]]; then
    blocker "Admin and Mail public origins must be distinct"
  fi
  admin_org="$(env_value_or_default COPILOTMAIL_ADMIN_ORGANIZATION_ID "")"
  mailbox_id="$(env_value_or_default COPILOTMAIL_MAIL_APP_MAILBOX_ID "")"
  uuid_value "${admin_org}" || blocker "Admin organization ID must be a UUID"
  if [[ -n "${mailbox_id}" ]]; then
    uuid_value "${mailbox_id}" || blocker "Mail mailbox preference must be a UUID when configured"
  fi
  session_ttl="$(env_value_or_default COPILOTMAIL_HUMAN_SESSION_TTL_SECONDS 3600)"
  if ! unsigned_integer "${session_ttl}" ||
    ((session_ttl < 300 || session_ttl > 28800)); then
    blocker "Human Session TTL must be between 300 and 28800 seconds"
  fi
fi

if ((WITH_MAIL || STALWART_BOOTSTRAP)); then
  require_nonsecret_env COPILOTMAIL_STALWART_PUBLIC_URL || true
  data_plane_id="$(env_value_or_default COPILOTMAIL_MAIL_DATA_PLANE_ID "018f305d-3d0f-7b2a-9d68-7f4a3f6e2101")"
  uuid_value "${data_plane_id}" || blocker "Mail data-plane ID must be a UUID"
  data_plane_origin="$(env_value_or_default COPILOTMAIL_MAIL_DATA_PLANE_ORIGIN "")"
  stalwart_url="$(env_value_or_default COPILOTMAIL_STALWART_PUBLIC_URL "")"
  [[ "${data_plane_origin}" == "${stalwart_url}" ]] ||
    blocker "Mail data-plane origin must exactly match the Stalwart public URL"
  if [[ "${ACCESS_MODE}" == "https" ]]; then
    https_origin "${stalwart_url}" ||
      blocker "HTTPS Pilot requires an origin-only HTTPS Stalwart public URL"
  elif [[ "${ACCESS_MODE}" == "ssh-tunnel" ]] &&
    [[ ! "${stalwart_url}" =~ ^http://host\.docker\.internal:[0-9]+$ &&
      ! "${stalwart_url}" =~ ^https:// ]]; then
    blocker "SSH-tunnel Stalwart URL must use host.docker.internal HTTP or HTTPS"
  fi
fi

check_secret_file() {
  local relative_path="$1"
  local required="$2"
  local minimum="$3"
  local maximum="$4"
  local exact="${5:-0}"
  local absolute_path="${REPO_ROOT}/deploy/secrets/${relative_path}"
  if [[ ! -e "${absolute_path}" && ! -L "${absolute_path}" ]]; then
    if ((required)); then
      blocker "required Secret file is missing: deploy/secrets/${relative_path}"
    else
      info "optional Secret is not present: deploy/secrets/${relative_path}"
    fi
    return
  fi
  if [[ -L "${absolute_path}" ]]; then
    blocker "Secret bind source must not be a symbolic link: deploy/secrets/${relative_path}"
    return
  fi
  if [[ ! -f "${absolute_path}" ]]; then
    blocker "Secret bind source must be a regular file: deploy/secrets/${relative_path}"
    return
  fi
  local size owner_uid mode
  size="$(stat -c '%s' "${absolute_path}" 2>/dev/null || true)"
  owner_uid="$(stat -c '%u' "${absolute_path}" 2>/dev/null || true)"
  mode="$(stat -c '%a' "${absolute_path}" 2>/dev/null || true)"
  if secret_size_valid "${size}" "${minimum}" "${maximum}" "${exact}"; then
    pass "Secret file size is valid: deploy/secrets/${relative_path}"
  else
    blocker "Secret file size is invalid: deploy/secrets/${relative_path}"
  fi
  if secret_mode_readable_by_control_uid "${owner_uid}" "${mode}"; then
    pass "Secret metadata proves UID ${CONTROL_UID} read access: deploy/secrets/${relative_path}"
  else
    blocker "Secret must be owned by UID ${CONTROL_UID}, owner-readable, non-executable, and inaccessible to group/other: deploy/secrets/${relative_path}"
  fi
}

check_secret_file human-session.key "${WITH_HUMAN_SESSION}" 32 32 32
mail_required="${WITH_MAIL}"
check_secret_file stalwart-jmap.token "${mail_required}" 1 16384
check_secret_file stalwart-mail-content.token "${mail_required}" 1 16384
check_secret_file stalwart-mail-compose.token "${mail_required}" 1 16384
check_secret_file stalwart-mail-submission.token "${mail_required}" 1 16384
check_secret_file stalwart-events.bearer "${mail_required}" 32 4098
check_secret_file stalwart-events.hmac "${mail_required}" 32 16386
check_secret_file audit-private.pem "${WITH_AUDIT_MAINTENANCE}" 1 1048576
check_secret_file audit-public.pem "${WITH_AUDIT_MAINTENANCE}" 1 1048576

if [[ "${DOCKER_SECURITY_OPTIONS}" == *rootless* ||
  "${DOCKER_SECURITY_OPTIONS}" == *userns* ]]; then
  if ((WITH_HUMAN_SESSION || WITH_MAIL || WITH_AUDIT_MAINTENANCE)); then
    blocker "rootless/userns mapping prevents a fail-closed static UID 10001 proof; add an explicit in-container read test"
  fi
fi

if ((docker_ready)) && [[ -f "${ENV_FILE}" ]]; then
  compose_args=(--env-file "${ENV_FILE}")
  for compose_file in "${selected_compose_files[@]}"; do
    compose_args+=(-f "${compose_file}")
  done
  if ((WITH_AUDIT_MAINTENANCE)); then
    compose_args+=(--profile audit-maintenance)
  fi

  clean_environment_args=(-i "PATH=${PATH}")
  for key in \
    HOME \
    XDG_RUNTIME_DIR \
    DOCKER_HOST \
    DOCKER_CONTEXT \
    DOCKER_TLS_VERIFY \
    DOCKER_CERT_PATH \
    DOCKER_CONFIG \
    SSH_AUTH_SOCK; do
    if [[ -v "${key}" ]]; then
      clean_environment_args+=("${key}=${!key}")
    fi
  done

  if (
    cd -- "${REPO_ROOT}"
    timeout 30s env "${clean_environment_args[@]}" \
      docker compose "${compose_args[@]}" config --format json 2>/dev/null |
      validate_compose_config_json
  ) >/dev/null 2>&1; then
    pass "sanitized V2 Compose configuration has the fixed Pilot name and loopback-only published ports"
  else
    blocker "sanitized V2 Compose configuration failed to render or violated Pilot project/port policy; inspect it without publishing rendered secrets"
  fi
fi

if ((STALWART_BOOTSTRAP && !WITH_MAIL)); then
  info "Stalwart bootstrap stage selected: only the stalwart service may be started"
fi
if ((WITH_AUDIT_MAINTENANCE)); then
  warn "static checks cannot prove the new audit-maintenance named volume is writable by UID 10001"
fi
if ((WITH_MAIL)); then
  warn "static checks cannot prove host.docker.internal reaches the loopback-bound Stalwart listener"
fi
if [[ "${ACCESS_MODE}" == "https" ]]; then
  warn "DNS, certificate trust, reverse-proxy routing, and Windows Firewall require external probes"
fi

printf 'SUMMARY: blockers=%d warnings=%d\n' "${BLOCKERS}" "${WARNINGS}"
if ((BLOCKERS > 0)); then
  exit 1
fi
printf '%s\n' \
  'Ubuntu/WSL Pilot preflight passed.' \
  'This result does not authorize production use or public MX cutover.'
