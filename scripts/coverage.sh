#!/usr/bin/env bash
# Rust test coverage, the same run as .github/workflows/coverage.yml.
#
#   scripts/coverage.sh            # all tests, HTML report in coverage/html
#   scripts/coverage.sh core::     # only tests matching a nextest filter
#
# Needs cargo-llvm-cov and cargo-nextest:
#   rustup component add llvm-tools-preview
#   cargo install --locked cargo-llvm-cov cargo-nextest
set -euo pipefail

root="$(cd "$(dirname "$0")/.." && pwd)"
cd "$root/src-tauri"

cargo llvm-cov nextest --locked --no-report "$@"
mkdir -p "$root/coverage"
cargo llvm-cov report --lcov --output-path "$root/coverage/lcov.info"
cargo llvm-cov report --html --output-dir "$root/coverage"
cargo llvm-cov report --summary-only | tail -n 1
echo "HTML report: $root/coverage/html/index.html"
