#!/usr/bin/env bash
set -euo pipefail

capture_path="${MGS_TEST_SMS_CAPTURE_PATH:?MGS_TEST_SMS_CAPTURE_PATH is required}"
message="${2:-}"
code="$(printf '%s' "$message" | grep -Eo '[0-9]{6}' | head -n1 || true)"

if [ -z "$code" ]; then
  echo "capture_sms.sh: no 6-digit OTP code found in SMS payload" >&2
  exit 1
fi

mkdir -p "$(dirname "$capture_path")"
printf '%s\n' "$code" > "$capture_path"
