#!/usr/bin/env bash
# Fails when a migration added on this branch sorts at or before the newest
# migration on the base ref. `supabase db push` refuses such a migration once
# the base's migrations are applied, so catch it before merge and renumber.
# Usage: scripts/check-migration-order.sh <base-ref>
set -euo pipefail
base="${1:?usage: $0 <base-ref>}"
dir=supabase/migrations

latest=$(git ls-tree --name-only "$base" "$dir/" | xargs -rn1 basename | sort | tail -1)
[ -z "$latest" ] && exit 0

status=0
while read -r f; do
  name=$(basename "$f")
  if [[ ! "$name" > "$latest" ]] || [[ "${name%%_*}" == "${latest%%_*}" ]]; then
    echo "::error file=$f::$name must sort after $latest, the newest migration on $base. Renumber it."
    status=1
  fi
done < <(git diff --name-only --diff-filter=A "$(git merge-base "$base" HEAD)" HEAD -- "$dir/")
exit $status
