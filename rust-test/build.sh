#!/usr/bin/env bash
set -euo pipefail

# Build the rust-test userspace program with the Seele toolchain
# and install it into the sysroot programs directory.

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
ROOT_DIR="$(cd "${SCRIPT_DIR}/.." && pwd)"

CRATE_NAME="rust-test"
TARGET_TRIPLE="x86_64-seele"
TOOLCHAIN="seele"

SYSROOT="${ROOT_DIR}/sysroot"
TARGET_DIR="${SYSROOT}/programs"

echo "[rust-test] building with toolchain '${TOOLCHAIN}' for target '${TARGET_TRIPLE}'..."
cd "${ROOT_DIR}"

cargo +${TOOLCHAIN} build --release --target "${TARGET_TRIPLE}" -p "${CRATE_NAME}"

BIN_PATH="${ROOT_DIR}/target/${TARGET_TRIPLE}/release/${CRATE_NAME}"
if [[ ! -f "${BIN_PATH}" ]]; then
    echo "[rust-test] error: built binary not found at ${BIN_PATH}" >&2
    exit 1
fi

echo "[rust-test] installing to ${TARGET_DIR}/${CRATE_NAME}..."
sudo mkdir -p "${TARGET_DIR}"
sudo rm -f "${TARGET_DIR}/${CRATE_NAME}"
sudo cp "${BIN_PATH}" "${TARGET_DIR}/${CRATE_NAME}"
sync

echo "[rust-test] done."
