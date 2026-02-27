#!/usr/bin/env bash

set -euo pipefail

DOMAIN="${1:-game.trebuchet.network}"
BASE_URL="https://$DOMAIN"

require_cmd() {
  local cmd="$1"
  if ! command -v "$cmd" >/dev/null 2>&1; then
    echo "error: required command '$cmd' is not installed." >&2
    exit 1
  fi
}

require_cmd curl

resolve_records() {
  local rr_type="$1"
  if command -v dig >/dev/null 2>&1; then
    dig +short "$rr_type" "$DOMAIN" | sed '/^$/d' | sort -u
    return
  fi

  if [[ "$rr_type" == "A" ]] && command -v getent >/dev/null 2>&1; then
    getent ahostsv4 "$DOMAIN" | awk '{print $1}' | sort -u
    return
  fi

  if [[ "$rr_type" == "AAAA" ]] && command -v getent >/dev/null 2>&1; then
    getent ahostsv6 "$DOMAIN" | awk '{print $1}' | sort -u
    return
  fi

  return 0
}

echo "== DNS =="
a_records="$(resolve_records A || true)"
aaaa_records="$(resolve_records AAAA || true)"

if [[ -z "$a_records" && -z "$aaaa_records" ]]; then
  echo "error: no A/AAAA records resolved for $DOMAIN" >&2
  exit 1
fi

if [[ -n "$a_records" ]]; then
  echo "A records:"
  echo "$a_records" | sed 's/^/  - /'
fi

if [[ -n "$aaaa_records" ]]; then
  echo "AAAA records:"
  echo "$aaaa_records" | sed 's/^/  - /'
fi

echo
echo "== HTTPS Endpoints =="

check_status() {
  local path="$1"
  local expected="$2"
  local code
  code="$(curl -sS -o /dev/null -w "%{http_code}" "$BASE_URL$path")"
  if [[ "$code" != "$expected" ]]; then
    echo "error: $BASE_URL$path returned HTTP $code (expected $expected)" >&2
    exit 1
  fi
  echo "ok: $path -> $code"
}

check_status "/" "302"
check_status "/index.html" "200"
check_status "/client.html" "200"
check_status "/ui-template.html" "200"

health_json="$(curl -fsS "$BASE_URL/healthz")"
ready_json="$(curl -fsS "$BASE_URL/readyz")"

if ! printf '%s' "$health_json" | rg -q '"ok"\s*:\s*true'; then
  echo "error: /healthz response does not contain ok=true" >&2
  echo "$health_json" >&2
  exit 1
fi
if ! printf '%s' "$ready_json" | rg -q '"ready"\s*:\s*true'; then
  echo "error: /readyz response does not contain ready=true" >&2
  echo "$ready_json" >&2
  exit 1
fi

echo "ok: /healthz and /readyz payload checks"

echo
echo "== TLS Certificate =="
if command -v openssl >/dev/null 2>&1; then
  cert_info="$(
    echo | openssl s_client -servername "$DOMAIN" -connect "$DOMAIN:443" 2>/dev/null \
      | openssl x509 -noout -subject -issuer -dates
  )"
  echo "$cert_info" | sed 's/^/  /'
else
  echo "warning: openssl not installed; skipping certificate inspection."
fi

echo
echo "Public deployment verification passed for $DOMAIN"
