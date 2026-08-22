#!/bin/sh
set -eu
validate_simple() {
  name="$1"; value="$2"
  line_count="$(printf '%s' "$value" | wc -l | tr -d ' ')"
  if [ "$line_count" != "0" ] || { [ -n "$value" ] && ! printf '%s' "$value" | grep -Eq '^[-A-Za-z0-9._:/?&=@+,% ]+$'; }; then
    echo "invalid characters in $name" >&2; exit 1
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
api="${COPILOTMAIL_MAIL_APP_API_BASE_URL:-}"
mailbox="${COPILOTMAIL_MAIL_APP_MAILBOX_ID:-}"
for pair in "API_BASE_URL:$api" "MAILBOX_ID:$mailbox"; do
  validate_simple "COPILOTMAIL_MAIL_APP_${pair%%:*}" "${pair#*:}"
done
validate_same_origin_path COPILOTMAIL_MAIL_APP_API_BASE_URL "$api"
cat > /tmp/runtime-config.js <<EOF
window.__COPILOTMAIL_MAIL_CONFIG__ = {
  apiBaseUrl: "$api", mailboxId: "$mailbox"
};
EOF
