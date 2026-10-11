#!/usr/bin/env bash
set -euo pipefail

readonly admin_image="copilotmail-admin:test"
readonly mail_image="copilotmail-mail:test"

for required_command in docker curl awk grep sed tr mktemp; do
  if ! command -v "$required_command" >/dev/null 2>&1; then
    printf 'required command is unavailable: %s\n' "$required_command" >&2
    exit 1
  fi
done

# Docker Engine releases before 28.0 had a documented localhost-publishing
# caveat that could expose loopback-bound ports to peers on the same L2 segment.
docker_server_version="$(docker version --format '{{.Server.Version}}')"
docker_server_major="${docker_server_version%%.*}"
if [[ ! "$docker_server_major" =~ ^[0-9]+$ ]] ||
  ((docker_server_major < 28)); then
  printf 'Docker Engine 28 or newer is required for strict loopback isolation; found %s\n' \
    "$docker_server_version" >&2
  exit 1
fi
readonly docker_server_version docker_server_major

suffix="$(
  printf '%s' "${GITHUB_RUN_ID:-local}-${GITHUB_RUN_ATTEMPT:-0}-$$" |
    tr -cd 'A-Za-z0-9_.-'
)"
suffix="${suffix:0:48}"
if [[ -z "$suffix" ]]; then
  printf 'could not derive a safe Docker resource suffix\n' >&2
  exit 1
fi
readonly suffix

readonly admin_container="copilotmail-admin-runtime-${suffix}"
readonly mail_container="copilotmail-mail-runtime-${suffix}"
readonly admin_network="copilotmail-admin-runtime-${suffix}"
readonly mail_network="copilotmail-mail-runtime-${suffix}"

admin_network_created=0
mail_network_created=0
admin_container_created=0
mail_container_created=0
admin_port=""
mail_port=""

scratch_parent="${RUNNER_TEMP:-${TMPDIR:-/tmp}}"
if [[ ! -d "$scratch_parent" ]]; then
  printf 'temporary directory parent does not exist: %s\n' "$scratch_parent" >&2
  exit 1
fi
scratch="$(mktemp -d "${scratch_parent%/}/copilotmail-web-runtime.XXXXXX")"
readonly scratch

finish() {
  local exit_code="$?"
  local resource=""
  local container=""
  local container_created="0"
  trap - EXIT INT TERM
  set +e

  if ((exit_code != 0)); then
    printf '%s\n' '::group::Web container runtime failure diagnostics'
    for resource in \
      "$admin_container:$admin_container_created" \
      "$mail_container:$mail_container_created"; do
      container="${resource%:*}"
      container_created="${resource##*:}"
      if ((container_created == 1)) &&
        docker container inspect "$container" >/dev/null 2>&1; then
        printf '\n--- %s state ---\n' "$container"
        docker container inspect \
          --format 'status={{.State.Status}} exit={{.State.ExitCode}} health={{if .State.Health}}{{.State.Health.Status}}{{else}}missing{{end}}' \
          "$container" || true
        printf '%s\n' "--- ${container} logs ---"
        docker container logs --tail 200 "$container" || true
      fi
    done
    printf '%s\n' '::endgroup::'
  fi

  if ((admin_container_created == 1)); then
    docker container rm --force "$admin_container" >/dev/null 2>&1 || true
  fi
  if ((mail_container_created == 1)); then
    docker container rm --force "$mail_container" >/dev/null 2>&1 || true
  fi
  if ((admin_network_created == 1)); then
    docker network rm "$admin_network" >/dev/null 2>&1 || true
  fi
  if ((mail_network_created == 1)); then
    docker network rm "$mail_network" >/dev/null 2>&1 || true
  fi
  rm -f -- "$scratch"/*.headers "$scratch"/*.body
  rmdir -- "$scratch" 2>/dev/null || true
  exit "$exit_code"
}
trap finish EXIT
trap 'exit 130' INT
trap 'exit 143' TERM

for image in "$admin_image" "$mail_image"; do
  docker image inspect "$image" >/dev/null
  # The controlled hosts entry lets nginx resolve its configured upstream
  # without providing or contacting a backend during this static config test.
  docker run --rm \
    --network none \
    --add-host api:127.0.0.1 \
    --entrypoint nginx \
    "$image" -t
done

docker network create --driver bridge --internal "$admin_network" >/dev/null
admin_network_created=1
docker network create --driver bridge --internal "$mail_network" >/dev/null
mail_network_created=1

docker container create \
  --name "$admin_container" \
  --network "$admin_network" \
  --add-host api:127.0.0.1 \
  --publish "127.0.0.1::8080" \
  --cap-drop ALL \
  --security-opt no-new-privileges:true \
  "$admin_image" >/dev/null
admin_container_created=1

docker container create \
  --name "$mail_container" \
  --network "$mail_network" \
  --add-host api:127.0.0.1 \
  --publish "127.0.0.1::8080" \
  --cap-drop ALL \
  --security-opt no-new-privileges:true \
  "$mail_image" >/dev/null
mail_container_created=1

docker container start "$admin_container" >/dev/null
docker container start "$mail_container" >/dev/null

loopback_port() {
  local container="$1"
  local binding
  binding="$(docker container port "$container" 8080/tcp)"
  if [[ ! "$binding" =~ ^127\.0\.0\.1:([0-9]{2,5})$ ]]; then
    printf '%s did not receive an IPv4 loopback-only port: %s\n' \
      "$container" "$binding" >&2
    return 1
  fi
  printf '%s\n' "${BASH_REMATCH[1]}"
}

admin_port="$(loopback_port "$admin_container")"
mail_port="$(loopback_port "$mail_container")"
if [[ "$admin_port" == "$mail_port" ]]; then
  printf 'Docker assigned the same host port to both isolated applications: %s\n' \
    "$admin_port" >&2
  exit 1
fi
readonly admin_port mail_port

wait_for_healthy() {
  local container="$1"
  local deadline=$((SECONDS + 75))
  local running=""
  local health=""

  while ((SECONDS < deadline)); do
    running="$(docker container inspect --format '{{.State.Running}}' "$container")"
    health="$(
      docker container inspect \
        --format '{{if .State.Health}}{{.State.Health.Status}}{{else}}missing{{end}}' \
        "$container"
    )"
    if [[ "$running" != "true" ]]; then
      printf 'container exited before becoming healthy: %s\n' "$container" >&2
      return 1
    fi
    if [[ "$health" == "healthy" ]]; then
      return 0
    fi
    sleep 1
  done

  printf 'container did not become healthy: %s (last health: %s)\n' \
    "$container" "$health" >&2
  return 1
}

wait_for_healthy "$admin_container"
wait_for_healthy "$mail_container"

fetch_response() {
  local label="$1"
  local port="$2"
  local path="$3"
  local expected_status="$4"
  local headers="$scratch/${label}.headers"
  local body="$scratch/${label}.body"
  local actual_status

  if [[ ! "$label" =~ ^[a-z0-9-]+$ ]] ||
    [[ ! "$port" =~ ^[0-9]{2,5}$ ]] ||
    [[ ! "$path" =~ ^/[A-Za-z0-9._/-]*$ ]] ||
    [[ ! "$expected_status" =~ ^[1-5][0-9]{2}$ ]]; then
    printf 'unsafe HTTP verification input: %s %s %s %s\n' \
      "$label" "$port" "$path" "$expected_status" >&2
    return 1
  fi
  if [[ "$path" == *".."* ]] || [[ "$path" == *"//"* ]]; then
    printf 'ambiguous HTTP verification path: %s\n' "$path" >&2
    return 1
  fi

  actual_status="$(
    curl \
      --silent \
      --show-error \
      --connect-timeout 3 \
      --max-time 10 \
      --dump-header "$headers" \
      --output "$body" \
      --write-out '%{http_code}' \
      "http://127.0.0.1:${port}${path}"
  )"
  if [[ "$actual_status" != "$expected_status" ]]; then
    printf '%s returned HTTP %s; expected %s\n' \
      "$path" "$actual_status" "$expected_status" >&2
    return 1
  fi
}

header_value() {
  local name="$1"
  local headers="$2"
  awk -F ':' -v expected_name="$name" '
    tolower($1) == tolower(expected_name) {
      sub(/^[^:]*:[[:space:]]*/, "")
      sub(/\r$/, "")
      value = $0
    }
    END { print value }
  ' "$headers"
}

assert_no_store() {
  local label="$1"
  local value
  value="$(header_value Cache-Control "$scratch/${label}.headers" | tr '[:upper:]' '[:lower:]')"
  if ! grep -Eq '(^|[[:space:],])no-store([[:space:],]|$)' <<<"$value"; then
    printf '%s Cache-Control is not no-store: %s\n' "$label" "$value" >&2
    return 1
  fi
}

assert_immutable() {
  local label="$1"
  local value
  value="$(header_value Cache-Control "$scratch/${label}.headers" | tr '[:upper:]' '[:lower:]')"
  for directive in public max-age=31536000 immutable; do
    if ! grep -Eq "(^|[[:space:],])${directive}([[:space:],]|$)" <<<"$value"; then
      printf '%s Cache-Control is missing %s: %s\n' "$label" "$directive" "$value" >&2
      return 1
    fi
  done
}

assert_html() {
  local label="$1"
  local content_type
  content_type="$(header_value Content-Type "$scratch/${label}.headers" | tr '[:upper:]' '[:lower:]')"
  if [[ "$content_type" != text/html* ]] ||
    ! grep -Eiq '<!doctype[[:space:]]+html|<html([[:space:]>])' "$scratch/${label}.body"; then
    printf '%s did not return an HTML document (Content-Type: %s)\n' \
      "$label" "$content_type" >&2
    return 1
  fi
}

verify_application() {
  local application="$1"
  local port="$2"
  local health_path="$3"
  local index_label="${application}-index"
  local runtime_label="${application}-runtime-config"
  local asset_label="${application}-asset"
  local missing_label="${application}-missing-asset"
  local fallback_label="${application}-spa-fallback"
  local health_label="${application}-health"
  local asset_path=""
  local asset_name=""
  local asset_content_type=""

  fetch_response "$index_label" "$port" / 200
  assert_no_store "$index_label"
  assert_html "$index_label"

  fetch_response "$runtime_label" "$port" /runtime-config.js 200
  assert_no_store "$runtime_label"

  asset_path="$(
    grep -Eo '/assets/[A-Za-z0-9._/-]+' "$scratch/${index_label}.body" |
      sed -n '1p' || true
  )"
  asset_name="${asset_path##*/}"
  if [[ ! "$asset_path" =~ ^/assets/[A-Za-z0-9][A-Za-z0-9._/-]*$ ]] ||
    [[ "$asset_path" == *".."* ]] ||
    [[ "$asset_path" == *"//"* ]] ||
    [[ ! "$asset_name" =~ -[A-Za-z0-9_-]{6,}\.[A-Za-z0-9]{1,8}$ ]]; then
    printf '%s index did not expose a safe content-hashed asset path: %s\n' \
      "$application" "$asset_path" >&2
    return 1
  fi
  fetch_response "$asset_label" "$port" "$asset_path" 200
  assert_immutable "$asset_label"
  asset_content_type="$(
    header_value Content-Type "$scratch/${asset_label}.headers" |
      tr '[:upper:]' '[:lower:]'
  )"
  if [[ "$asset_content_type" == text/html* ]]; then
    printf '%s hashed asset unexpectedly returned HTML\n' "$application" >&2
    return 1
  fi

  fetch_response \
    "$missing_label" "$port" "/assets/copilotmail-ci-missing-${suffix}.js" 404
  assert_no_store "$missing_label"

  fetch_response \
    "$fallback_label" "$port" "/ci-spa-fallback-${suffix}/deep-link" 200
  assert_no_store "$fallback_label"
  assert_html "$fallback_label"

  fetch_response "$health_label" "$port" "$health_path" 200
  if [[ "$(tr -d '\r\n' <"$scratch/${health_label}.body")" != "ok" ]]; then
    printf '%s health endpoint returned an unexpected body\n' "$application" >&2
    return 1
  fi
}

verify_application admin "$admin_port" /admin-healthz
verify_application mail "$mail_port" /mail-healthz

printf '%s\n' \
  'web container runtime ok: nginx config, loopback-only serving, cache policy, SPA fallback, and health checks verified'
