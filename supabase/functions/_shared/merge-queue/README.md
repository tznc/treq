# Merge Queue — Architecture & Operations

Asynchronous, durable merge-queue backend built on Supabase Queues (PGMQ).

```
GitHub
  │ webhook
  ▼
github-webhook (Edge Function)
  1. Verify HMAC signature (before JSON parsing)
  2. Deduplicate delivery via github_webhook_receipts
  3. Upsert directly observable facts (installations/repos)
  4. Publish domain commands to PGMQ
  5. Return 202
  │
  ▼
PGMQ: merge_queue_commands ──(failures)──▶ merge_queue_dead_letters
  │
  ▼
merge-queue-worker (Edge Function, cron + webhook nudge)
  1. Read batch with visibility timeout
  2. Claim command execution (command_id idempotency)
  3. Acquire per-queue execution lease
  4. Re-read authoritative domain state
  5. Apply transition + GitHub side effects (operation-key idempotent)
  6. Archive on success / backoff retry / dead-letter
  │
  ├── GitHub API (via github-adapter)
  └── PostgreSQL domain tables (source of truth)

merge-queue-reconciler (Edge Function, cron)
  Repairs drift by publishing corrective commands.
```

**Architectural rule:** PGMQ messages represent work that must be
*attempted*. The domain tables (`merge_queues`, `merge_queue_entries`,
`ci_runs`, `ci_run_entries`, configs) are the authoritative state; every
command is reconstructable from them and carries no credentials or full
webhook payloads.

## Identity model

| Identity | Purpose | Enforced by |
|---|---|---|
| `github_delivery_id` | GitHub ingress dedupe | `github_webhook_receipts` PK + payload hash check |
| `command_id` | internal command dedupe | `merge_queue_command_executions` PK |
| `operation_key` | logical action dedupe (e.g. `merge-pr:{repo}:{pr}:{testedSha}`) | partial unique index on succeeded executions |

## Concurrency model

- PGMQ `read` applies a visibility timeout (120s), so an in-flight message
  is not delivered to a second consumer.
- All mutations of a given queue additionally require the queue's
  **execution lease** (`merge_queue_execution_leases`, 120s TTL). Two
  workers can progress *different* queues concurrently; never the same one.
- Lane reservation (`reserve_next_merge_queue_lane`) is a single
  transaction that validates the lease, locks the queue row, selects the
  batch deterministically and creates `ci_runs` + `ci_run_entries`.

## Merge safety

- A PR merges only if it is still open, still targets the configured
  branch, and its head SHA equals the SHA tested in the lane
  (`ci_run_entries.tested_head_sha`). The expected SHA is also passed to
  GitHub's merge API, which rejects with 409 if the head moved.
- `required_checks` (when configured) are evaluated against check runs on
  the exact tested SHA; `neutral`/`skipped` do **not** count as success.
  With no required checks, only an explicit `success` suite conclusion
  passes.
- A moved head re-queues the entry at the new SHA instead of failing it.

## Pro entitlement

The merge queue is a Pro feature (`prds/billing-and-teams.md`,
"Enforcement"). `installation_has_pro(installation_id)` answers for the
installation's owner: today the user who linked it.

- `set_merge_queue_enabled(..., true)` and any write that turns
  `merge_queue_configs.enabled` on raise SQLSTATE `PT402` (HTTP 402, hint
  `pro_required`) without Pro. Turning the queue off always works.
- `enqueue-workspace` answers 402 `pro_required` to `enqueue`; `dequeue`
  always works.
- Before `queue.drive` or `ci.completed` tests or merges anything, the
  worker checks the owner (`entitlement-pause.ts`). Without Pro it cancels
  every lane, puts its PRs back to `queued`, and comments once on each
  waiting PR (operation key and marker `pro-paused:{entryId}`). Later drives
  of a paused queue only read the database. When Pro returns, the
  reconciler's next drive starts lanes again.

## Retry & dead-letter policy

- Retryable (GitHub 429/5xx/rate-limit 403, network, lease contention,
  checks still pending): message rescheduled via `set_vt` with backoff
  15s → 60s → 5m → 15m; dead-letter after 5 attempts.
- Terminal (unknown message version, malformed payload, other 4xx,
  missing repo/queue): dead-lettered immediately with a sanitised error
  envelope. Successful messages are **archived**, never deleted.

## Deployment

```sh
# 1. Database (enables pgmq, creates queues/tables/RPCs)
supabase db push

# 2. Functions
supabase functions deploy github-webhook            # verify_jwt=false (HMAC-authenticated)
supabase functions deploy merge-queue-worker
supabase functions deploy merge-queue-reconciler
supabase functions deploy enqueue-workspace
supabase functions deploy create-github-install-intent
supabase functions deploy complete-github-installation

# 3. Secrets
supabase secrets set \
  GITHUB_APP_ID=... \
  GITHUB_APP_PRIVATE_KEY_BASE64=... \
  GITHUB_APP_CLIENT_ID=... \
  GITHUB_APP_CLIENT_SECRET=... \
  GITHUB_WEBHOOK_SECRET=...
```

`GITHUB_WEBHOOK_SECRET` is mandatory: the webhook function refuses every
request when it is unset. Never log tokens, the private key, the client
secret, or the webhook secret.

### GitHub App settings for installation linking

GitHub warns that the `installation_id` on its post-install redirect can be
forged, so `complete-github-installation` only links an installation that
the installing GitHub user can access. In the App's settings
(**Developer settings → GitHub Apps → Edit**, on the account or
organization that owns the App):

- Under **Identifying and authorizing users**, select **Request user
  authorization (OAuth) during installation**, and set the first
  **Callback URL** to `https://treq.dev/integrations/github/callback`.
  With this option on, **Setup URL** cannot be set and GitHub redirects to the
  callback URL with `code`, `installation_id`, `setup_action` and the
  `state` passed to the install URL.
- Copy the **Client ID** into `GITHUB_APP_CLIENT_ID`, and generate a client
  secret (**Client secrets → Generate a new client secret**) for
  `GITHUB_APP_CLIENT_SECRET`.

The function exchanges `code` for a user access token, checks the
installation is in that user's `GET /user/installations` (up to 10 pages of
100), and discards the token. A redirect without `code` answers 400
`github_authorization_required`, so an App without the setting fails closed.

### Cron

Correctness does not depend on the webhook's fire-and-forget worker nudge;
cron is the recovery mechanism. Schedule with pg_cron + pg_net (Dashboard →
Integrations → Cron), storing the URL/service key in Vault:

```sql
select cron.schedule(
  'merge-queue-worker', '* * * * *',   -- every minute (or finer where supported)
  $$ select net.http_post(
       url := (select decrypted_secret from vault.decrypted_secrets where name = 'project_url')
              || '/functions/v1/merge-queue-worker',
       headers := jsonb_build_object(
         'Content-Type', 'application/json',
         'Authorization', 'Bearer ' ||
           (select decrypted_secret from vault.decrypted_secrets where name = 'service_role_key')),
       body := '{"reason":"cron"}'::jsonb) $$);

select cron.schedule(
  'merge-queue-reconciler', '*/10 * * * *',  -- every 10 minutes
  $$ select net.http_post(
       url := (select decrypted_secret from vault.decrypted_secrets where name = 'project_url')
              || '/functions/v1/merge-queue-reconciler',
       headers := jsonb_build_object(
         'Content-Type', 'application/json',
         'Authorization', 'Bearer ' ||
           (select decrypted_secret from vault.decrypted_secrets where name = 'service_role_key')),
       body := '{}'::jsonb) $$);
```

## Observability

- `select * from public.merge_queue_metrics();` — queue depth and oldest
  visible message age for both queues (service role).
- `public.merge_queue_stuck_ci_runs` — active runs older than 2h.
- `public.merge_queue_command_failures` — failed executions with attempt
  counts and last errors.
- Worker/webhook logs are structured JSON carrying `command_id`,
  `pgmq_message_id`, `github_delivery_id`, `queue_id`, `worker_id`,
  `attempt`, `operation`, `duration_ms`, `outcome`.

Replaying a dead letter: read it from `pgmq.q_merge_queue_dead_letters`,
fix the underlying cause, then `select pgmq.send('merge_queue_commands',
<original command jsonb>);` with a fresh `commandId`.

## Security posture

- Browser roles have **no** access to PGMQ wrappers, receipts, executions,
  leases or install intents (RLS enabled, no policies; wrapper EXECUTE
  revoked from `anon`/`authenticated`).
- Installation linking requires a server-minted single-use intent
  (`create-github-install-intent` → GitHub install with `state` →
  `complete-github-installation` verifies user + expiry, exchanges GitHub's
  OAuth `code`, requires the installation in that GitHub user's
  `GET /user/installations`, and looks it up with app auth before
  linking). A browser-supplied `installation_id` alone never determines
  ownership.
