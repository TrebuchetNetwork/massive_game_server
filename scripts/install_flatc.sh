#!/usr/bin/env bash

set -euo pipefail

FLATC_VERSION="${FLATC_VERSION:-v25.2.10}"
OS="$(uname -s)"
ARCH="$(uname -m)"

case "$OS" in
    Linux)
        ASSET_NAME="Linux.flatc.binary.g++-13.zip"
        ;;
    Darwin)
        if [[ "$ARCH" == "x86_64" ]]; then
            ASSET_NAME="MacIntel.flatc.binary.zip"
        else
            ASSET_NAME="Mac.flatc.binary.zip"
        fi
        ;;
    *)
        echo "error: unsupported OS for flatc install: $OS" >&2
        exit 1
        ;;
esac

TMP_DIR="$(mktemp -d)"
trap 'rm -rf "$TMP_DIR"' EXIT

DOWNLOAD_URL="https://github.com/google/flatbuffers/releases/download/${FLATC_VERSION}/${ASSET_NAME}"
curl -fsSL "$DOWNLOAD_URL" -o "$TMP_DIR/flatc.zip"
unzip -q "$TMP_DIR/flatc.zip" -d "$TMP_DIR"

mkdir -p "$HOME/.local/bin"
install -m 0755 "$TMP_DIR/flatc" "$HOME/.local/bin/flatc"

if [[ -n "${GITHUB_PATH:-}" ]]; then
    echo "$HOME/.local/bin" >> "$GITHUB_PATH"
fi
export PATH="$HOME/.local/bin:$PATH"

flatc --version
