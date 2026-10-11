#!/usr/bin/env bash
# Runs tests/sprites_e2e.rs against a local Supabase CLI stack whose Edge
# Functions call the real Sprites API (no stub). See SPRITES_E2E.md.
#
#   TREQ_SPRITES_API_TOKEN=<test-org token> mobile-gpui/scripts/sprites-e2e-local.sh
#
# Set SKIP_STACK=1 to reuse a stack this script already started.
set -euo pipefail

root="$(cd "$(dirname "$0")/../.." && pwd)"
cd "$root"

: "${TREQ_SPRITES_API_TOKEN:?set TREQ_SPRITES_API_TOKEN to a token from a dedicated Sprites test organization}"
for tool in supabase docker cargo; do
	command -v "$tool" >/dev/null 2>&1 || { echo "$tool not found" >&2; exit 1; }
done

if [ "${SKIP_STACK:-}" != "1" ]; then
	docker info >/dev/null 2>&1 || { echo "Docker is not reachable" >&2; exit 1; }
	npm install --omit=dev --prefix supabase/functions >/dev/null

	# The functions read this file when the stack starts. Unlike
	# scripts/service-qa/up.sh there is no TREQ_REMOTE_SPRITES_STUB, so
	# provisioning hits Sprites. The relay URL must be one this machine can
	# dial: the functions' own SUPABASE_URL is a Docker-internal address.
	# The CA pair is the local-only one service-qa uses.
	env_file=supabase/functions/.env
	[ -f "$env_file" ] && cp "$env_file" "$env_file.bak-sprites-e2e"
	cat >"$env_file" <<EOF
TREQ_SPRITES_API_TOKEN=${TREQ_SPRITES_API_TOKEN}
TREQ_SPRITES_API_URL=${TREQ_SPRITES_API_URL:-https://api.sprites.dev}
REMOTE_SSH_RELAY_URL=http://127.0.0.1:54321/functions/v1/remote-ssh-relay
REMOTE_SSH_CA_ED25519_SEED_BASE64=U7QP4DsQ6pPuNz9nyuniR6tEsHqTOvulJhpWElYm07Y=
REMOTE_SSH_CA_ED25519_PUBLIC_KEY_BASE64=uL88vOSTAX5b7HkQyciJmsjAKWT4aJVnukznoRrgDic=
EOF
	echo "Wrote $env_file (previous copy, if any: $env_file.bak-sprites-e2e)"

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
