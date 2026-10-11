#!/usr/bin/env bash
# Runs tests/sprites_e2e.rs against a local Supabase CLI stack whose Edge
# Functions call the real Sprites API (no stub). See SPRITES_E2E.md.
#
# You configure supabase/functions/.env yourself; this script only checks it.
#
#   mobile-gpui/scripts/sprites-e2e-local.sh
#
# Set SKIP_STACK=1 to reuse a stack that is already running with that .env.
set -euo pipefail

root="$(cd "$(dirname "$0")/../.." && pwd)"
cd "$root"

for tool in supabase docker cargo; do
	command -v "$tool" >/dev/null 2>&1 || { echo "$tool not found" >&2; exit 1; }
done

env_file=supabase/functions/.env
fail() { echo "$env_file: $1 (see mobile-gpui/SPRITES_E2E.md)" >&2; exit 1; }
[ -f "$env_file" ] || fail "missing"
grep -Eq '^TREQ_SPRITES_API_TOKEN=.+' "$env_file" || fail "set TREQ_SPRITES_API_TOKEN to a test-organization token"
grep -Eq '^REMOTE_SSH_RELAY_URL=.+' "$env_file" || fail "set REMOTE_SSH_RELAY_URL so this machine can dial the relay"
grep -Eq '^REMOTE_SSH_CA_ED25519_SEED_BASE64=.+' "$env_file" || fail "set the REMOTE_SSH_CA_ED25519_* pair"
grep -Eq '^TREQ_REMOTE_SPRITES_STUB=1' "$env_file" && fail "remove TREQ_REMOTE_SPRITES_STUB=1, or provisioning never reaches Sprites"

if [ "${SKIP_STACK:-}" != "1" ]; then
	docker info >/dev/null 2>&1 || { echo "Docker is not reachable" >&2; exit 1; }
	npm install --omit=dev --prefix supabase/functions >/dev/null
	# The functions read the .env when the stack starts.
	supabase stop >/dev/null 2>&1 || true
	supabase start --exclude logflare,vector,studio,imgproxy,storage-api
	supabase db reset --local
fi

eval "$(supabase status -o env | grep -E '^(API_URL|ANON_KEY|SERVICE_ROLE_KEY)=')"
export TREQ_REMOTE_E2E=1 TREQ_REMOTE_E2E_MOBILE=1
export SUPABASE_TEST_URL="$API_URL" SUPABASE_TEST_ANON_KEY="$ANON_KEY" SUPABASE_TEST_SERVICE_ROLE_KEY="$SERVICE_ROLE_KEY"

# treq_lib embeds the desktop frontend, which the mobile app does not use.
[ -f src/dist/index.html ] || { mkdir -p src/dist && echo '<!doctype html>' >src/dist/index.html; }

cd mobile-gpui
cargo test --test sprites_e2e -- --ignored --nocapture --test-threads=1
