#!/usr/bin/env bash
# Repository-wide style + lint gate. Mirrors what CI runs.
#
# Steps:
#   1. `cargo fmt --all -- --check`  — formatting matches `rustfmt.toml`.
#   2. `cargo clippy --workspace --all-targets -- -D warnings`
#      — workspace lints in `Cargo.toml` are enforced.
#   3. `cargo test --workspace --quiet`  — tests pass.
#
# Usage:  scripts/check.sh
# Exit status is the first failing step.

set -euo pipefail

SCRIPT_DIR="$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")" &>/dev/null && pwd)"
cd "${SCRIPT_DIR}/.."

echo "==> cargo fmt --check"
cargo fmt --all -- --check

echo "==> cargo clippy"
cargo clippy --workspace --all-targets -- -D warnings

echo "==> cargo test"
cargo test --workspace --quiet

echo "OK: style, lints, and tests all pass."
