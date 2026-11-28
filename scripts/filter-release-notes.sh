#!/usr/bin/env bash
# Drops chore, ci, build, test, refactor, lint and style PRs, plus feat(web),
# feat(lint), fix(web), fix(lint), fix(deps), fix(e2e) and fix(test) PRs, from
# GitHub's generated release notes.
# Reads the notes on stdin and writes the filtered notes to stdout.
set -euo pipefail

sed -E '/^\* ((chore|ci|build|test|refactor|lint|style)(\([^)]*\))?|feat\((web|lint)\)|fix\((web|lint|deps|e2e|test)\))!?: /d'
