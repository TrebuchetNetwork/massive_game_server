#!/usr/bin/env bash

set -euo pipefail

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
CANONICAL_SCHEMA="$ROOT_DIR/protocol/schemas/game.fbs"
SERVER_SCHEMA_MIRROR="$ROOT_DIR/server/schemas/game.fbs"
CLIENT_GENERATED_DIR="$ROOT_DIR/static_client/generated_js"

if ! command -v flatc >/dev/null 2>&1; then
    echo "error: flatc is not installed" >&2
    exit 1
fi

if [ ! -f "$CANONICAL_SCHEMA" ]; then
    echo "error: missing canonical schema at $CANONICAL_SCHEMA" >&2
    exit 1
fi
if [ ! -f "$SERVER_SCHEMA_MIRROR" ]; then
    echo "error: missing server schema mirror at $SERVER_SCHEMA_MIRROR" >&2
    exit 1
fi

if ! cmp -s "$CANONICAL_SCHEMA" "$SERVER_SCHEMA_MIRROR"; then
    echo "error: schema drift between canonical and mirror copies" >&2
    diff -u "$CANONICAL_SCHEMA" "$SERVER_SCHEMA_MIRROR" || true
    exit 1
fi

tmp_dir="$(mktemp -d)"
trap 'rm -rf "$tmp_dir"' EXIT

flatc --ts -o "$tmp_dir" "$CANONICAL_SCHEMA"

generated_list="$tmp_dir/generated_ts_files.txt"
checked_in_list="$tmp_dir/checked_in_ts_files.txt"
(cd "$tmp_dir" && find . -type f -name '*.ts' ! -name '*.d.ts' | sort) > "$generated_list"
(cd "$CLIENT_GENERATED_DIR" && find . -type f -name '*.ts' ! -name '*.d.ts' | sort) > "$checked_in_list"

if ! diff -u "$generated_list" "$checked_in_list" >/dev/null; then
    echo "error: generated file set differs from checked-in .ts file set" >&2
    diff -u "$generated_list" "$checked_in_list" || true
    exit 1
fi

while IFS= read -r rel_path; do
    if ! cmp -s "$tmp_dir/$rel_path" "$CLIENT_GENERATED_DIR/$rel_path"; then
        echo "error: stale generated client FlatBuffers file: $CLIENT_GENERATED_DIR/$rel_path" >&2
        echo "run: scripts/generate_flatbuffers.sh --skip-tsc" >&2
        exit 1
    fi
done < "$generated_list"

echo "FlatBuffers schema + generated TypeScript consistency check passed."
