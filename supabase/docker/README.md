# Treq Supabase fat API image (Alpine, single-tenant)

Alpine-based image with the Supabase **API** processes for single-tenant
self-hosted treq. **No GoTrue** — there is no `/auth/v1` user login surface.

| Process | Role |
|---|---|
| PostgREST | REST + RPC (`/rest/v1`) |
| Edge Runtime | Deno functions (`/functions/v1`) |
| nginx | Kong-compatible public gateway (`:8000`) |

Process supervision is **OpenRC** (`supervise-daemon`). Runtime image has no
Python, no Node.js, and no supervisord. Functions import `@supabase/supabase-js`
via its `npm:` specifier, resolved by edge-runtime at request time (no
`node_modules`, no bundling). `edge-runtime` is stripped and UPX-packed
(`upx -9`) in a discarded build stage.

Postgres is a compose sidecar (`supabase/postgres`). Use `SERVICE_ROLE_KEY` as
the API credential (bypasses RLS); there are no end-user sessions.

## Quick start

```bash
make supabase.docker.build
make supabase.docker.up
make supabase.docker.smoke
```

API: `http://127.0.0.1:54321`  
Postgres: `postgresql://postgres:postgres@127.0.0.1:54322/postgres`

## CI / full verification

```bash
make supabase.docker.test
```

This brings up `docker-compose.test.yml` and runs
[`test/verify-self-hosted.sh`](./test/verify-self-hosted.sh), which checks:

- Gateway `/health` (and that `/auth/v1` is absent)
- Data API: OpenAPI, `profiles` read
- Edge: `exchange-desktop-token`, `github-webhook` HMAC

GitHub Actions: `.github/workflows/supabase-docker.yml`.

## Environment

See [`.env.example`](./.env.example). Required: `PGHOST`, `POSTGRES_PASSWORD`,
`JWT_SECRET`, `ANON_KEY`, `SERVICE_ROLE_KEY`. Default `VERIFY_JWT=false`.

## Version pins

See [versions.env](./versions.env).
