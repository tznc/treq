#!/usr/bin/env bash
# Drops chore, ci, build, test, lint, style, feat(web) and fix(lint) PRs from
# GitHub's generated release notes.
# Reads the notes on stdin and writes the filtered notes to stdout.
set -euo pipefail

sed -E '/^\* ((chore|ci|build|test|lint|style)(\([^)]*\))?|feat\(web\)|fix\(lint\))!?: /d'
