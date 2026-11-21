#!/usr/bin/env bash
# Drops chore, lint and style PRs from GitHub's generated release notes.
# Reads the notes on stdin and writes the filtered notes to stdout.
set -euo pipefail

sed -E '/^\* (chore|lint|style)(\([^)]*\))?!?: /d'
