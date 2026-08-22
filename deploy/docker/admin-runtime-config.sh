#!/bin/sh
set -eu

validate_simple() {
  name="$1"
  value="$2"
  line_count="$(printf '%s' "$value" | wc -l | tr -d ' ')"
  if [ "$line_count" != "0" ] ||
    { [ -n "$value" ] && ! printf '%s' "$value" | grep -Eq '^[-A-Za-z0-9._:/?&=@+,% ]+$'; }; then
    echo "invalid characters in $name" >&2
    exit 1
  fi
}

validate_same_origin_path() {
  case "$2" in
    ""|/*) ;;
    *)
      echo "$1 must be empty or a same-origin absolute path" >&2
      exit 1
      ;;
  esac
  case "$2" in
    //*)
      echo "$1 must not be a protocol-relative URL" >&2
      exit 1
      ;;
  esac
}

api_base_url="${COPILOTMAIL_ADMIN_API_BASE_URL:-}"
organization_id="${COPILOTMAIL_ADMIN_ORGANIZATION_ID:-}"

validate_simple COPILOTMAIL_ADMIN_API_BASE_URL "$api_base_url"
validate_same_origin_path COPILOTMAIL_ADMIN_API_BASE_URL "$api_base_url"
validate_simple COPILOTMAIL_ADMIN_ORGANIZATION_ID "$organization_id"

cat > /tmp/runtime-config.js <<EOF
window.__COPILOTMAIL_CONFIG__ = {
  apiBaseUrl: "$api_base_url",
  organizationId: "$organization_id"
};
EOF
