#!/usr/bin/env bash

set -euo pipefail

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
tracked_files_list="$(mktemp)"
trap 'rm -f "$tracked_files_list"' EXIT
git -C "$ROOT_DIR" ls-files > "$tracked_files_list"

blocked_patterns=(
    '(^|/)\.DS_Store$'
    '(^|/)panic\.log$'
    '(^|/)runtime_server\.log$'
    '\.zip$'
    '^(data|server/data|test-results)(/|$)'
)

has_violations=0
for pattern in "${blocked_patterns[@]}"; do
    matches="$(rg -N "$pattern" "$tracked_files_list" || true)"
    if [ -n "$matches" ]; then
        has_violations=1
        printf 'tracked artifact hygiene violation (%s):\n' "$pattern" >&2
        printf '%s\n' "$matches" | sed 's/^/  /' >&2
    fi
done

if [ "$has_violations" -ne 0 ]; then
    exit 1
fi

echo "Repository hygiene check passed."
