#!/usr/bin/env bash
set -euo pipefail

if [[ $# -lt 1 ]]; then
  echo "usage: $0 <target-triple>" >&2
  exit 1
fi

TARGET_TRIPLE="$1"
OUTPUT_DIR="${OUTPUT_DIR:-dist}"
PACKAGE_VERSION="${PACKAGE_VERSION:-dev}"

BIN_EXT=""
if [[ "$TARGET_TRIPLE" == *"windows"* ]]; then
  BIN_EXT=".exe"
fi

BIN_DIR="target/${TARGET_TRIPLE}/release"
SOCAT_BIN="${BIN_DIR}/socat${BIN_EXT}"
SOCAT_RS_BIN="${BIN_DIR}/socat-rs${BIN_EXT}"

if [[ ! -f "$SOCAT_BIN" || ! -f "$SOCAT_RS_BIN" ]]; then
  echo "missing binaries under ${BIN_DIR}" >&2
  exit 1
fi

PKG_NAME="socat-rs-${PACKAGE_VERSION}-${TARGET_TRIPLE}"
STAGE_DIR="${OUTPUT_DIR}/${PKG_NAME}"
ARCHIVE_PATH="${OUTPUT_DIR}/${PKG_NAME}.tar.gz"
CHECKSUM_PATH="${ARCHIVE_PATH}.sha256"

rm -rf "$STAGE_DIR"
mkdir -p "$STAGE_DIR/docs"

cp "$SOCAT_BIN" "$STAGE_DIR/"
cp "$SOCAT_RS_BIN" "$STAGE_DIR/"
cp README.md README.zh-CN.md "$STAGE_DIR/"
cp docs/FEATURE_STATUS.en.md docs/FEATURE_STATUS.zh-CN.md "$STAGE_DIR/docs/"
cp docs/V1_READY.en.md docs/V1_READY.zh-CN.md "$STAGE_DIR/docs/"
cp docs/compatibility-roadmap.md "$STAGE_DIR/docs/"

tar -C "$OUTPUT_DIR" -czf "$ARCHIVE_PATH" "$PKG_NAME"

if command -v sha256sum >/dev/null 2>&1; then
  sha256sum "$ARCHIVE_PATH" > "$CHECKSUM_PATH"
else
  shasum -a 256 "$ARCHIVE_PATH" > "$CHECKSUM_PATH"
fi

echo "created: ${ARCHIVE_PATH}"
echo "checksum: ${CHECKSUM_PATH}"
