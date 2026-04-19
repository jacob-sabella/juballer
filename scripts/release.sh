#!/usr/bin/env bash
# Build a Linux x86_64 release tarball for juballer-deck.
#
# Produces dist/juballer-<version>-linux-x86_64.tar.gz containing:
#   - the juballer-deck binary (release profile: LTO + stripped)
#   - assets/
#   - README.md
#
# Usage: scripts/release.sh
set -euo pipefail

# Resolve repo root (the parent of this script's directory).
SCRIPT_DIR="$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")" &>/dev/null && pwd)"
REPO_ROOT="$(cd -- "${SCRIPT_DIR}/.." &>/dev/null && pwd)"
cd "${REPO_ROOT}"

# Pull the workspace version from the top-level Cargo.toml. It's the
# single source of truth via workspace.package.version.
VERSION="$(
    awk '
        /^\[workspace\.package\]/ { in_wp = 1; next }
        /^\[/ && !/^\[workspace\.package\]/ { in_wp = 0 }
        in_wp && /^version[[:space:]]*=/ {
            gsub(/"/, "", $3)
            print $3
            exit
        }
    ' Cargo.toml
)"
if [[ -z "${VERSION}" ]]; then
    echo "release.sh: could not determine workspace version from Cargo.toml" >&2
    exit 1
fi

TARGET_TRIPLE="linux-x86_64"
STAGE_NAME="juballer-${VERSION}-${TARGET_TRIPLE}"
TARBALL="dist/${STAGE_NAME}.tar.gz"

echo "release.sh: building juballer-deck v${VERSION} (release profile)"
cargo build --release -p juballer-deck

BIN_PATH="target/release/juballer-deck"
if [[ ! -x "${BIN_PATH}" ]]; then
    echo "release.sh: expected binary at ${BIN_PATH} but it wasn't produced" >&2
    exit 1
fi

STAGE_DIR="dist/${STAGE_NAME}"
rm -rf "${STAGE_DIR}" "${TARBALL}"
mkdir -p "${STAGE_DIR}"

cp "${BIN_PATH}" "${STAGE_DIR}/juballer-deck"
cp README.md "${STAGE_DIR}/README.md"
if [[ -f LICENSE ]]; then
    cp LICENSE "${STAGE_DIR}/LICENSE"
fi
# Bundle the sample charts + audio so the tarball is runnable out of the box.
cp -r assets "${STAGE_DIR}/assets"

# Produce the tarball with the stage dir as the top-level entry.
tar -C dist -czf "${TARBALL}" "${STAGE_NAME}"
rm -rf "${STAGE_DIR}"

echo "release.sh: wrote ${REPO_ROOT}/${TARBALL}"
