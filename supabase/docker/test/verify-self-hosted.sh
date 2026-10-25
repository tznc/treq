#!/usr/bin/env bash
# Verify treq Supabase fat API in single-tenant self-hosted mode (no GoTrue).
# Expects the stack from docker-compose.test.yml on http://127.0.0.1:54321.
set -euo pipefail

BASE="${SUPABASE_URL:-http://127.0.0.1:54321}"
ANON="${ANON_KEY:-eyJhbGciOiJIUzI1NiIsInR5cCI6IkpXVCJ9.eyJpc3MiOiJzdXBhYmFzZS1kZW1vIiwicm9sZSI6ImFub24iLCJleHAiOjE5ODM4MTI5OTZ9.CRXP1A7WOeoJeXxjNni43kdQwgnWNReilDMblYTn_I0}"
SERVICE="${SERVICE_ROLE_KEY:-eyJhbGciOiJIUzI1NiIsInR5cCI6IkpXVCJ9.eyJpc3MiOiJzdXBhYmFzZS1kZW1vIiwicm9sZSI6InNlcnZpY2Vfcm9sZSIsImV4cCI6MTk4MzgxMjk5Nn0.EGIM96RAZx35lJzdJsyH-qQwv8Hdp7fsn3W0YpN81IU}"
WEBHOOK_SECRET="${GITHUB_WEBHOOK_SECRET:-ci-test-webhook-secret}"

pass=0
fail=0

assert_http() {
  local name="$1"
  local expected="$2"
  local got="$3"
  if [ "${got}" = "${expected}" ]; then
    echo "PASS  ${name} (HTTP ${got})"
    pass=$((pass + 1))
  else
    echo "FAIL  ${name} (expected HTTP ${expected}, got ${got})"
    fail=$((fail + 1))
  fi
}

assert_true() {
  local name="$1"
  shift
  if "$@"; then
    echo "PASS  ${name}"
    pass=$((pass + 1))
  else
    echo "FAIL  ${name}"
    fail=$((fail + 1))
  fi
}

echo "=== treq self-hosted single-tenant verification ==="
echo "BASE=${BASE}"
echo

# ── Gateway ──────────────────────────────────────────────────────────────────
echo "-- gateway --"
code="$(curl -s -o /tmp/treq-health.json -w '%{http_code}' "${BASE}/health")"
assert_http "GET /health" "200" "${code}"
assert_true "health body is ok" grep -q '"status":"ok"' /tmp/treq-health.json

# GoTrue must not be present.
code="$(curl -s -o /dev/null -w '%{http_code}' "${BASE}/auth/v1/health")"
assert_true "GET /auth/v1/health is absent (no GoTrue)" \
  test "${code}" = "404" -o "${code}" = "502" -o "${code}" = "000"

# ── Data API (PostgREST) — service_role bypasses RLS in single-tenant ────────
echo "-- data API --"
code="$(curl -s -o /tmp/treq-rest-openapi.json -w '%{http_code}' \
  -H "apikey: ${SERVICE}" \
  -H "Authorization: Bearer ${SERVICE}" \
  "${BASE}/rest/v1/")"
assert_http "GET /rest/v1/ OpenAPI (service_role)" "200" "${code}"

code="$(curl -s -o /tmp/treq-profiles.json -w '%{http_code}' \
  -H "apikey: ${SERVICE}" \
  -H "Authorization: Bearer ${SERVICE}" \
  -H "Accept: application/json" \
  "${BASE}/rest/v1/profiles?select=id&limit=1")"
assert_http "GET /rest/v1/profiles (service_role)" "200" "${code}"
assert_true "profiles response is JSON array" grep -q '^\[' /tmp/treq-profiles.json

# Anon can still hit OpenAPI (role=anon JWT); table RLS may empty-filter.
code="$(curl -s -o /tmp/treq-rest-anon.json -w '%{http_code}' \
  -H "apikey: ${ANON}" \
  -H "Authorization: Bearer ${ANON}" \
  "${BASE}/rest/v1/")"
assert_http "GET /rest/v1/ OpenAPI (anon)" "200" "${code}"

# ── Edge Functions (VERIFY_JWT=false — no user session required) ─────────────
echo "-- edge functions --"

code="$(curl -s -o /tmp/treq-fn-exchange.json -w '%{http_code}' \
  -X POST "${BASE}/functions/v1/exchange-desktop-token" \
  -H "Content-Type: application/json" \
  -H "apikey: ${ANON}" \
  -d '{}')"
assert_http "POST exchange-desktop-token without token" "400" "${code}"
assert_true "exchange-desktop-token reports missing token" \
  grep -qi 'token' /tmp/treq-fn-exchange.json

# github-webhook: function boots and rejects bad HMAC (secret is set).
body='{"action":"ping","zen":"ci"}'
sig="sha256=$(printf '%s' "${body}" | openssl dgst -sha256 -hmac "${WEBHOOK_SECRET}" | awk '{print $2}')"
code="$(curl -s -o /tmp/treq-fn-webhook.json -w '%{http_code}' \
  -X POST "${BASE}/functions/v1/github-webhook" \
  -H "Content-Type: application/json" \
  -H "X-GitHub-Event: ping" \
  -H "X-GitHub-Delivery: 00000000-0000-4000-8000-000000000001" \
  -H "X-Hub-Signature-256: ${sig}" \
  -d "${body}")"
assert_true "POST github-webhook with valid HMAC reaches function (not 502)" \
  test "${code}" != "502" -a "${code}" != "000"
echo "      github-webhook HTTP ${code}"

# Bad signature must fail closed.
code="$(curl -s -o /tmp/treq-fn-webhook-bad.json -w '%{http_code}' \
  -X POST "${BASE}/functions/v1/github-webhook" \
  -H "Content-Type: application/json" \
  -H "X-GitHub-Event: ping" \
  -H "X-GitHub-Delivery: 00000000-0000-4000-8000-000000000002" \
  -H "X-Hub-Signature-256: sha256=deadbeef" \
  -d "${body}")"
assert_true "POST github-webhook with bad HMAC is rejected" \
  test "${code}" = "401" -o "${code}" = "403"

echo
echo "=== results: ${pass} passed, ${fail} failed ==="
if [ "${fail}" -ne 0 ]; then
  exit 1
fi
echo "verify-self-hosted ok"
