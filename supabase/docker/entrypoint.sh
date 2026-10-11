#!/usr/bin/env bash
# Wait for external Postgres, apply treq migrations, hand off to OpenRC.
# Single-tenant self-hosted: no GoTrue. No supervisord / Python.
set -euo pipefail

log() { printf '[entrypoint] %s\n' "$*"; }

: "${POSTGRES_PASSWORD:?POSTGRES_PASSWORD is required}"
: "${JWT_SECRET:?JWT_SECRET is required}"
: "${ANON_KEY:?ANON_KEY is required}"
: "${SERVICE_ROLE_KEY:?SERVICE_ROLE_KEY is required}"
: "${PGHOST:?PGHOST is required (external Postgres hostname)}"

export POSTGRES_DB="${POSTGRES_DB:-postgres}"
export POSTGRES_USER="${POSTGRES_USER:-supabase_admin}"
export PGPORT="${PGPORT:-5432}"
export POSTGRES_PORT="${POSTGRES_PORT:-${PGPORT}}"
export JWT_EXP="${JWT_EXP:-3600}"
export PGPASSWORD="${POSTGRES_PASSWORD}"
export PGUSER="${POSTGRES_USER}"
export PGDATABASE="${POSTGRES_DB}"

# Public URL clients use to reach this container (mapped host port).
export SUPABASE_PUBLIC_URL="${SUPABASE_PUBLIC_URL:-http://127.0.0.1:8000}"

# Internal loopback URL used by Edge Functions talking back to the gateway.
export SUPABASE_URL="${SUPABASE_URL:-http://127.0.0.1:8000}"
export SUPABASE_ANON_KEY="${ANON_KEY}"
export SUPABASE_SERVICE_ROLE_KEY="${SERVICE_ROLE_KEY}"
export SUPABASE_DB_URL="${SUPABASE_DB_URL:-postgresql://postgres:${POSTGRES_PASSWORD}@${PGHOST}:${PGPORT}/${POSTGRES_DB}}"

# PostgREST → external Postgres (env-driven; no config file)
export PGRST_DB_URI="postgres://authenticator:${POSTGRES_PASSWORD}@${PGHOST}:${PGPORT}/${POSTGRES_DB}"
export PGRST_DB_SCHEMAS="${PGRST_DB_SCHEMAS:-public,graphql_public}"
export PGRST_DB_EXTRA_SEARCH_PATH="${PGRST_DB_EXTRA_SEARCH_PATH:-public,extensions}"
export PGRST_DB_ANON_ROLE=anon
export PGRST_DB_MAX_ROWS="${PGRST_DB_MAX_ROWS:-1000}"
export PGRST_JWT_SECRET="${JWT_SECRET}"
export PGRST_DB_USE_LEGACY_GUCS=false
export PGRST_APP_SETTINGS_JWT_SECRET="${JWT_SECRET}"
export PGRST_APP_SETTINGS_JWT_EXP="${JWT_EXP}"

# Edge Runtime — single-tenant defaults to no gateway JWT checks.
export JWT_SECRET
export VERIFY_JWT="${VERIFY_JWT:-false}"
export GITHUB_WEBHOOK_SECRET="${GITHUB_WEBHOOK_SECRET:-}"
export GITHUB_APP_ID="${GITHUB_APP_ID:-}"
export GITHUB_APP_PRIVATE_KEY_BASE64="${GITHUB_APP_PRIVATE_KEY_BASE64:-}"
export GITHUB_APP_CLIENT_ID="${GITHUB_APP_CLIENT_ID:-}"
export GITHUB_APP_CLIENT_SECRET="${GITHUB_APP_CLIENT_SECRET:-}"
export MERGE_QUEUE_GITHUB_STUB="${MERGE_QUEUE_GITHUB_STUB:-0}"
export DENO_DIR="${DENO_DIR:-/root/.cache/deno}"
export FUNCTIONS_HTTP_PORT="${FUNCTIONS_HTTP_PORT:-9000}"
export KONG_HTTP_PORT="${KONG_HTTP_PORT:-8000}"
export MERGE_QUEUE_WORKER_INTERVAL_SEC="${MERGE_QUEUE_WORKER_INTERVAL_SEC:-60}"
export MERGE_QUEUE_RECONCILER_INTERVAL_SEC="${MERGE_QUEUE_RECONCILER_INTERVAL_SEC:-600}"

mkdir -p /tmp /run/openrc
touch /run/openrc/softlevel

psql_admin() {
  psql -v ON_ERROR_STOP=1 --no-psqlrc \
    -h "${PGHOST}" -p "${PGPORT}" -U "${POSTGRES_USER}" -d "${POSTGRES_DB}" "$@"
}

wait_for_postgres() {
  local ready_count=0
  local i
  log "waiting for Postgres at ${PGHOST}:${PGPORT}…"
  for i in $(seq 1 240); do
    if psql_admin -tAc "select 1" >/dev/null 2>&1 \
      && psql_admin -tAc "select 1 from pg_roles where rolname = 'authenticator'" 2>/dev/null | grep -q 1; then
      ready_count=$((ready_count + 1))
      if [ "${ready_count}" -ge 3 ]; then
        log "Postgres is ready"
        return 0
      fi
    else
      ready_count=0
    fi
    sleep 1
  done
  log "ERROR: Postgres at ${PGHOST}:${PGPORT} did not become ready in time"
  return 1
}

set_role_password() {
  local role="$1"
  local exists
  exists="$(psql_admin -tAc "select 1 from pg_roles where rolname = '${role}'" || true)"
  if [ "${exists}" = "1" ]; then
    psql_admin -c "alter role \"${role}\" with password '${POSTGRES_PASSWORD}'"
  else
    log "skip password for missing role ${role}"
  fi
}

wait_for_postgres

log "applying role passwords + JWT settings…"
for role in authenticator pgbouncer supabase_auth_admin supabase_storage_admin supabase_functions_admin postgres; do
  set_role_password "${role}"
done
psql_admin -f /opt/treq/db/jwt.sql

log "applying treq migrations…"
/opt/treq/bin/apply-treq-migrations.sh

# Persist env for OpenRC wrappers (supervise-daemon does not inherit entrypoint env).
mkdir -p /etc/treq
{
  printf 'export POSTGRES_PASSWORD=%q\n' "${POSTGRES_PASSWORD}"
  printf 'export POSTGRES_DB=%q\n' "${POSTGRES_DB}"
  printf 'export POSTGRES_USER=%q\n' "${POSTGRES_USER}"
  printf 'export PGHOST=%q\n' "${PGHOST}"
  printf 'export PGPORT=%q\n' "${PGPORT}"
  printf 'export PGUSER=%q\n' "${PGUSER}"
  printf 'export PGDATABASE=%q\n' "${PGDATABASE}"
  printf 'export PGPASSWORD=%q\n' "${PGPASSWORD}"
  printf 'export JWT_SECRET=%q\n' "${JWT_SECRET}"
  printf 'export JWT_EXP=%q\n' "${JWT_EXP}"
  printf 'export ANON_KEY=%q\n' "${ANON_KEY}"
  printf 'export SERVICE_ROLE_KEY=%q\n' "${SERVICE_ROLE_KEY}"
  printf 'export SUPABASE_PUBLIC_URL=%q\n' "${SUPABASE_PUBLIC_URL}"
  printf 'export SUPABASE_URL=%q\n' "${SUPABASE_URL}"
  printf 'export SUPABASE_ANON_KEY=%q\n' "${SUPABASE_ANON_KEY}"
  printf 'export SUPABASE_SERVICE_ROLE_KEY=%q\n' "${SUPABASE_SERVICE_ROLE_KEY}"
  printf 'export SUPABASE_DB_URL=%q\n' "${SUPABASE_DB_URL}"
  printf 'export PGRST_DB_URI=%q\n' "${PGRST_DB_URI}"
  printf 'export PGRST_DB_SCHEMAS=%q\n' "${PGRST_DB_SCHEMAS}"
  printf 'export PGRST_DB_EXTRA_SEARCH_PATH=%q\n' "${PGRST_DB_EXTRA_SEARCH_PATH}"
  printf 'export PGRST_DB_ANON_ROLE=%q\n' "${PGRST_DB_ANON_ROLE}"
  printf 'export PGRST_DB_MAX_ROWS=%q\n' "${PGRST_DB_MAX_ROWS}"
  printf 'export PGRST_JWT_SECRET=%q\n' "${PGRST_JWT_SECRET}"
  printf 'export PGRST_DB_USE_LEGACY_GUCS=%q\n' "${PGRST_DB_USE_LEGACY_GUCS}"
  printf 'export PGRST_APP_SETTINGS_JWT_SECRET=%q\n' "${PGRST_APP_SETTINGS_JWT_SECRET}"
  printf 'export PGRST_APP_SETTINGS_JWT_EXP=%q\n' "${PGRST_APP_SETTINGS_JWT_EXP}"
  printf 'export VERIFY_JWT=%q\n' "${VERIFY_JWT}"
  printf 'export GITHUB_WEBHOOK_SECRET=%q\n' "${GITHUB_WEBHOOK_SECRET}"
  printf 'export GITHUB_APP_ID=%q\n' "${GITHUB_APP_ID}"
  printf 'export GITHUB_APP_PRIVATE_KEY_BASE64=%q\n' "${GITHUB_APP_PRIVATE_KEY_BASE64}"
  printf 'export GITHUB_APP_CLIENT_ID=%q\n' "${GITHUB_APP_CLIENT_ID}"
  printf 'export GITHUB_APP_CLIENT_SECRET=%q\n' "${GITHUB_APP_CLIENT_SECRET}"
  printf 'export MERGE_QUEUE_GITHUB_STUB=%q\n' "${MERGE_QUEUE_GITHUB_STUB}"
  printf 'export DENO_DIR=%q\n' "${DENO_DIR}"
  printf 'export FUNCTIONS_HTTP_PORT=%q\n' "${FUNCTIONS_HTTP_PORT}"
  printf 'export KONG_HTTP_PORT=%q\n' "${KONG_HTTP_PORT}"
  printf 'export MERGE_QUEUE_WORKER_INTERVAL_SEC=%q\n' "${MERGE_QUEUE_WORKER_INTERVAL_SEC}"
  printf 'export MERGE_QUEUE_RECONCILER_INTERVAL_SEC=%q\n' "${MERGE_QUEUE_RECONCILER_INTERVAL_SEC}"
  printf 'export PATH=/usr/local/sbin:/usr/local/bin:/usr/sbin:/usr/bin:/sbin:/bin\n'
} >/etc/treq/env
chmod 600 /etc/treq/env

log "starting OpenRC default runlevel…"
openrc default

log "services up; idling (OpenRC supervise-daemon owns workers)"
exec sleep infinity
