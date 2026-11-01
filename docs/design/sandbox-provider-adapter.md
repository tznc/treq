# Pro cloud workspaces: providers, usage billing and migration

Scope: the hosted control plane in `supabase/`. Schema: migrations 044–051.

## Overview

A Pro user picks a **size tier**. Treq places the workspace on the cheapest admitted provider for that tier and region, and bills usage pay-as-you-go at **provider cost × 1.2** through a Stripe meter. Pro (US$15/month) stays the gate; there is no included allowance. Users never pick or see the provider, except when they bring their own key (BYOS).

Machines are disposable. Only the repo list (`remote_repositories`, with `remote_url`) and synced configs follow a workspace to a new machine. Unpushed commits, dirty working copies, agent logs, `.treq/local.db`, PTY state and caches are lost on a move, and preflight warns first.

## Components

- **Adapters** (`_shared/remote/*-adapter.ts`) implement `SandboxProvider` (`provider.ts`): create, start, stop, destroy, get, exec, `gatewayEndpoint`, plus a `capabilities` descriptor. `provider-registry.ts` resolves a provider by kind and placement, so a BYOS placement gets an adapter built with its owner's key. Vendor types and states never leave the adapter; errors map to `ProviderError.kind`.
- **Edge Functions:** `remote-instance` (the owner's API: ensure, wake, stop, resize, delete, process pins, migration acknowledgement, BYOS keys), `remote-ssh-relay` (ssh-over-WSS to the active placement), `remote-admin` (operator and maintenance actions), `stripe-webhook`.
- **Scheduling:** there is no pg_cron. `.github/workflows/remote-maintenance.yml` calls `remote-admin` on a schedule: `report_usage` hourly, `advance_migrations` + `destroy_retired_placements` hourly, `enforce_idle` every 5 min, `enforce_spend_caps` every 15 min, `sweep_lapsed_instances` hourly.
- **Postgres** holds the catalog, placements, ledger, migrations and their invariants. Every write goes through `security definer` functions; customer-facing ones check `auth.uid()`, the rest call `treq_internal.require_service_role()`.

## Providers

| Kind | Selectable | Gateway path | Notes |
|---|---|---|---|
| `boat` | yes | `POST /sandboxes/{id}/host` private URL + `Cookie: _port_auth`; gateway on `127.0.0.1:8022` | EU only (`eu_west`); cheapest there, so the Pro default |
| `daytona` | s/m | preview URL for port 8022 + `x-daytona-preview-token` | targets `us`/`eu` (no `ap_southeast`); l/xl hidden until account limits are raised; bills stopped storage |
| `e2b` | yes | `POST /v2/sandboxes/{id}/connect` → `wss://8022-<id>.<domain>/` + `e2b-traffic-access-token` | one template per tier (`<TREQ_E2B_TEMPLATE>-<tier>`); `autoPause` |
| `fly_machines` | **no** | `wss://<app>.fly.dev/` + `fly-force-instance-id`; gateway on `0.0.0.0:8022` | autostop off (Fly Proxy closes WebSockets to stop), so only the idle controller stops it; one shared Fly app, so tenants share a 6PN network; hidden until it uses an app per owner |
| `local` | dev only | none | in-process fake (`TREQ_REMOTE_PROVIDER_STUB`) |

Admission requires per-second billing and an authenticated path to the machine's gateway. Prices are seeded in 044 from each vendor's published list price (2026-10-27/28) in nano-USD per second. Daytona and E2B tie in every tier; the tie breaks by machine type id, so `daytona:*` is the default outside the EU and the fallback when Boat drains.

**Gateway transport.** Every provider speaks one protocol to the relay (`gateway-proxy.ts`): a text frame `{"host","port"}`, a reply `{"status":"connected"}` or an error, then raw binary frames. Inside the machine, `treq gateway` (a mode of the `treq` CLI from `crates/treq-gateway`, installed by the boot manifest) accepts only `127.0.0.1|localhost:2222`, treq's sshd. Each connection carries `x-treq-gateway-token`: `v1.<placement_id>.<expiry>.<hex HMAC-SHA256>` at most 120 s ahead, keyed by a per-placement secret kept in Vault (043) and written to `/etc/treq/gateway.secret`. sshd stays loopback-only and checks Treq CA certificates. `openTcp()` (`provider-tcp.ts`) resolves the active placement per connection, and on failure wakes the machine (re-running the pidfile-supervised sshd and gateway) and retries once.

## Data model

- **Catalog (036):** `sandbox_providers` (status `active | draining | retiring | retired`, health), `size_tiers` (`s/m/l/xl` = 2/4, 4/8, 8/16, 16/32 vCPU/GiB), `provider_machine_types`, `machine_type_prices` (versioned, never updated in place, no overlapping ranges), `pro_provider_defaults` (from `recompute_pro_defaults()`). Customers see prices only through `get_machine_catalog()`, already marked up.
- **Placements (046):** `remote_instances` stays the customer identity (stable id, endpoint, host key). Each machine is a `remote_instance_placements` row with role `active | candidate | retiring | dead`; at most one active and one candidate per instance. The instance mirrors the active placement and carries a `fence` bumped on every cutover. `disk_quota_gb` follows the machine type.
- **Ledger (046):** `usage_intervals` (`running` or `stopped_storage`, price frozen at open, append and close only), `usage_reports` (one row per owner-hour, the Stripe meter event).
- **Migrations (047):** `sandbox_migrations` + `sandbox_migration_steps`.
- **Spend cap (048), idle controller (049), config sync (050), gateway secrets (051), BYOS credentials (045).**

## Usage billing

- `set_placement_state(placement, state, disk_gb, at)` changes state and the ledger in one transaction. An active running placement has one open `running` interval; a suspended one has a `stopped_storage` interval only when its price bills stopped disk (disk defaults to the machine type's). `deleting` keeps billing until the provider confirms. `at` is the provider's transition time, clamped to the last ledger entry and `now()`. Candidates and retiring sources are not billed: failed creates, migration targets before cutover and the 24 h reverse-cutover window are treq's cost.
- A price change splits open intervals at `effective_from` (`split_repriced_intervals`), from `set_machine_type_price`, `set_placement_state` and `build_usage_reports`.
- `build_usage_reports(until)` buckets intervals into owner-hours for hours that ended at least 5 minutes earlier, sums nano-USD and rounds once per row. `report_usage` sends each row to Stripe meter `treq_sandbox_usage` with the row key as `identifier`. `no_customer` and `dead` are terminal so a bad row stops blocking newer ones.
- `treq_internal.billed_nanousd(cost, markup_bps, mode)` is the one pricing rule: managed bills `cost × (1 + markup)`, BYOS bills `cost × treq.byos_markup_bps` (default 5%). Reports, the spend cap and both catalogs use it.
- `get_usage_summary(from, to)` shows the owner hourly seconds and billed USD (with the BYOS part); provider cost and markup stay hidden.
- `enforce_idle` reconciles each placement against the provider and records drift through `set_placement_state`, so billing follows what the provider actually did.

### Spend cap

Every Pro user has a cap: US$50 by default, minimum US$5, maximum `treq.spend_cap_max_usd` (default US$1000), set with `set_spend_cap`. Spend counts the user's live Pro billing period from its first full hour (Stripe puts a partial first hour on the previous invoice), else the UTC calendar month: finished `usage_reports` plus unreported hours priced from the ledger. `enforce_spend_caps` sends one `spend_cap_warning` notice at 80% and one `spend_cap_reached` notice at 100% per period, stops running workspaces and rolls back open resizes. `remote-instance` refuses `ensure`, `wake` and `resize` with 402 `spend_cap_reached`. Raising the cap unblocks at once. Notices live in `user_notifications`; there is no email. A workspace can overshoot by up to about 15 minutes.

### Idle controller

`enforce_idle` probes each running workspace through `exec` (`activity-probe.ts`) and stops it after `TREQ_IDLE_TIMEOUT_MIN` (15) without human or agent activity: an sshd session, an agent turn whose log moved, a terminal with recent I/O, or a pinned process. Background processes never count unless pinned (`remote_process_pins`, by pid + command or substring, billed). A failed probe is not activity, but stopping on failures needs 3 in a row. A migration that builds or cuts over to a target exempts the workspace. Provider auto-stop (`TREQ_PROVIDER_AUTOSTOP_MIN`, 60) is the backstop and is extended on activity; Fly has none. `get_workspace_activity()` gives the app the processes, pins and `stops_at`.

### Bring your own sandbox (BYOS)

A Pro user saves their own provider key (Boat, Daytona, E2B) and runs on their own account; treq bills 5% of list price. The desktop `byos` feature preview is the only flag; `treq_internal.user_can_byos()` is the plan gate. Keys live in Vault (`remote_provider_credentials`, one per owner and kind); owners list metadata only. `remote-instance` validates a key with one read-only call before saving it. A 401/403 marks it invalid and the app sees `provider_credential_invalid`; treq never falls back to its own account. A trigger keeps a BYOS workspace on BYOS placements with its owner's credential, and operator drains skip BYOS placements. A key in use by a live placement cannot be removed.

## Provider switches

A switch happens on a resize to a tier whose provider differs, on an operator drain or retire, or on a cheaper default. It is always a fresh machine behind the same instance id, endpoint and host key.

```
requested → preflight → [awaiting_ack] → target_provisioning → target_bootstrapped
  → verifying → cutover → source_retiring → completed
failures before cutover → rolling_back → rolled_back (target destroyed)
```

- States change only through `advance_migration` (compare-and-swap) or `advance_migration_leased` for the worker holding the lease (`claim_sandbox_migrations`, `renew_…`, `release_…`). The hourly run and an inline resize share these leases.
- **Preflight** wakes the source and reports, per repo, dirty workspaces, commits on no remote and running agents. With unpushed work the migration parks in `awaiting_ack` until the owner acknowledges (`acknowledge_migration`) or an operator forces it (`force_migration`, audited). Preflight runs again first; pushed work clears the need for the ack.
- **Bootstrap** installs sshd, the host key and the gateway, re-clones repos from `remote_url` (repos without one are flagged `needs_attention`) and pulls configs.
- **Cutover** (`cutover_migration`) requires the fence captured at creation and the source still active; one transaction swaps roles, bumps the fence, moves billing and mirrors the instance. Relay sockets close and the client reconnects with the host key unchanged.
- The source stays stopped for 24 h. `reverse_cutover` moves back in one transaction while it is still retiring and no other migration is open. `destroy_retired_placements` then destroys it and any `orphaned_placements`.
- `cancel_open_migration` cancels from any state in one statement, so a delete or lapse sweep cannot lose to a worker.

## Operator controls (`remote-admin`)

`set_price` (new price version, splits intervals, recomputes defaults), `recompute_defaults`, `drain_provider` and `retire_provider` (provider becomes `draining`/`retiring`; each hourly run requests the next batch of migrations, capped by `TREQ_MIGRATION_MAX_OPEN_PER_PROVIDER` (10) and `TREQ_MIGRATION_MAX_OPEN` (50); a retiring provider with no live placement becomes `retired`), `force_migration`, `reverse_cutover`, `recover_instance`. A drain past its deadline stays parked and is reported as overdue, never forced. Operator actions are audited, with no owner when they touch no single user.

## Config sync

`put_user_config(scope, expected_version, files)` stores a versioned snapshot per scope (`app` or `repo:<remote_url>`), compare-and-swap on the version, files inline as base64 JSON up to 1 MiB, last 20 versions kept. The desktop app (`core/config_sync.rs`) and new placements read with `get_user_config` / `list_user_config_scopes`. Only an allowlist syncs: app settings as JSON, `skills/`, setup scripts and repo `.treq` config. Credentials, tokens, `*.db`, agent logs, PTY state and workspaces never sync.

## Decisions

- **Start fresh on a move, no backups.** Moving `.jj`/`.treq` state or bundles of unpushed work was a data path to build and test; preflight and the ack cover the loss.
- **Relay dials a gateway in the machine; no reverse tunnel or per-workspace DNS.** A tunnel needs a long-lived broker outside Supabase; DNS couples to TTLs and providers.
- **treq-gateway ships inside the `treq` binary.** One artifact the boot manifest already installs; no separate gateway release.
- **Fly Sprites dropped.** It billed hourly and could not run the gateway; it had no production users.
- **Fly Machines onboarded but hidden.** One shared app would let tenants reach each other over 6PN.
- **Own ledger, not provider usage APIs.** Provider APIs differ, lag or don't exist; intervals change with our own state transitions.
- **One Stripe meter with pre-multiplied amounts.** A meter per provider × machine type would change Stripe config with every catalog change.
- **Cheapest default computed per tier and region, ties by id.** No hand-maintained table.
- **Pay-as-you-go with no allowance, no prepaid credits.** Simplest model that tracks cost; the spend cap bounds surprises.
- **Spend cap stops workspaces, US$50 default.** With no allowance, notify-only would leave bills unbounded.
- **Stopped storage is billed where the provider bills it.** Absorbing it would make idle Daytona workspaces a loss.
- **Pro only.** The Team plan is being removed separately.
- **Maintenance runs from GitHub Actions.** The project has no pg_cron; leases make overlapping runs safe.
- **BYOS bills 5% of list price, gated in one SQL function.** The gate can move to a higher plan without code changes elsewhere.

## Open / unverified

- WebSocket upgrades with binary frames through every provider's proxy (Boat private host, Daytona preview, E2B port proxy, Fly with `fly-force-instance-id`).
- E2B: traffic-token header name, whether the proxy reaches a loopback listener, the `timeout` cap per plan, whether a `setsid` child outlives the envd call, per-template disk sizes, l/xl template limits, paused-storage price, region choice.
- Daytona: region support for all four regions (the adapter does not set `target`).
- Fly: m/l/xl prices scaled from performance-2x; non-iad regions cost more; an app per owner before it can be selectable.
- Boat: whether $0.018/h is a list or prepaid-plan rate, overage terms, API coverage and reliability.
- Monthly reconciliation of the ledger against provider invoices is not built.
- Nightly end-to-end switch (Boat → Daytona with a Stripe test-mode meter event) is not built.
- Multiple workspaces per user: `remote_instances` still allows one live instance per owner.
