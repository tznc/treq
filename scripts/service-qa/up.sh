#!/usr/bin/env bash
# Bring up the local Supabase CLI stack for service-qa.
# Nested Docker (Cloud Agent VMs) often needs vfs storage + bridge netfilter
# disabled so containers on the supabase bridge can reach each other.
set -euo pipefail

root="$(cd "$(dirname "$0")/../.." && pwd)"
cd "$root"

if ! command -v supabase >/dev/null 2>&1; then
	echo "supabase CLI not found. Install: https://supabase.com/docs/guides/local-development/cli/getting-started" >&2
	exit 1
fi

if ! docker info >/dev/null 2>&1; then
	echo "Docker is not reachable. Start the daemon (service-qa needs it)." >&2
	exit 1
fi

# Inter-container TCP on custom bridges fails when bridge-nf-call-iptables=1
# and Docker's DOCKER chain DROPs unmatched traffic (common in nested Docker).
if [ -w /proc/sys/net/bridge/bridge-nf-call-iptables ] 2>/dev/null; then
	echo 0 >/proc/sys/net/bridge/bridge-nf-call-iptables || true
	echo 0 >/proc/sys/net/bridge/bridge-nf-call-ip6tables || true
elif command -v sysctl >/dev/null 2>&1; then
	sysctl -w net.bridge.bridge-nf-call-iptables=0 >/dev/null 2>&1 || true
	sysctl -w net.bridge.bridge-nf-call-ip6tables=0 >/dev/null 2>&1 || true
fi

# Edge Functions resolve `npm:@supabase/supabase-js@2` from this local install
# (Deno nodeModulesDir), so workers do not need registry access at boot.
npm install --omit=dev --prefix supabase/functions

# Local-only Edge secrets for webhook HMAC + stubs (not for production).
# Loaded automatically from supabase/functions/.env on `supabase start`.
cat > supabase/functions/.env <<'EOF'
GITHUB_WEBHOOK_SECRET=service-qa-local-webhook-secret
TREQ_REMOTE_PROVIDER_STUB=1
REMOTE_SSH_CA_ED25519_SEED_BASE64=U7QP4DsQ6pPuNz9nyuniR6tEsHqTOvulJhpWElYm07Y=
REMOTE_SSH_CA_ED25519_PUBLIC_KEY_BASE64=uL88vOSTAX5b7HkQyciJmsjAKWT4aJVnukznoRrgDic=
STRIPE_WEBHOOK_SECRET=whsec_service_qa_local
GTM_METRICS_SECRET=service-qa-local-gtm-metrics-secret
EOF

# Skip analytics/studio/storage — not required for Auth/RPC/Edge service-qa.
excludes=(logflare vector studio imgproxy storage-api)

supabase start --exclude "$(IFS=,; echo "${excludes[*]}")"
supabase db reset --local

echo "service-qa stack ready. Run: npm run service-qa"
