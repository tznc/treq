# Testing the managed flow against real Sprites

`remote_e2e_native.rs` runs the managed-instance flow against real Fly Sprites. Desktop and mobile share this flow: both call the same `remote-instance` and `remote-ssh-trust` Edge Functions, and both connect through the same `treq_lib` relay transport, typed commands, PTY channel and credential cutoff. The only per-client difference is where the private key lives (desktop: a key file, mobile: the OS keystore). The suite keeps it in a temporary file.

## What one run does

1. Creates an e2e-tagged test user, grants it Pro (`ensure`, `wake` and `reprovision` answer 402 without it) and signs in.
2. Checks that `status` returns no instance, calls `ensure`, which provisions a real Sprite, and polls `status` until it is `ready` (up to 10 minutes).
3. Registers a client key, replays the registration with the same idempotency key and checks that it resolves to the same key.
4. Issues a certificate and checks that the endpoint pins host keys and routes SSH through the relay.
5. Over the relay: `InitRepo` and `InspectRepository` on two repositories, `CreateWorkspace` and `ListWorkspaces`, a PTY opened in the repository, and `AgentStatus`.
6. Renews the certificate (`renewal: true`), checks that the endpoint id stays the same and expiry does not go backwards, and runs a command on the same pool.
7. Reconnects on a fresh pool. Then it leaves the Sprite idle until it pauses, calls `wake`, checks that the VM was not recreated, and checks that the repository is still there.
8. Calls `reprovision`, re-issues the certificate and checks host trust: same generation means the same host keys, otherwise the generation must go up. Then it checks that the repository survived.
9. Revokes the key. It checks that renewal is refused with 409 "revoked" (what both clients map to a key-revoked cutoff), that a cut-off pool refuses commands locally, and that `report_cutoff` is accepted.
10. On exit, pass or panic, it deletes the instance (and so the Sprite), the billing rows and the user.

Expect a run to take about 5 to 15 minutes: provisioning plus the idle wait (`TREQ_REMOTE_E2E_IDLE_PAUSE_WAIT_SECS`, default 90).

## Prerequisites

- **A Sprites token for a dedicated test organization.** Sprites tokens are organization-scoped, so never use the production token. Create it at sprites.dev under the test organization.
- Rust stable, plus the desktop app's build dependencies.
- For option A: Docker and the [Supabase CLI](https://supabase.com/docs/guides/local-development/cli/getting-started).

## Option A: local Supabase stack with real Sprites (recommended)

You only need a Sprites token. Everything else runs on your machine.

1. Edit `supabase/functions/.env` yourself. The script reads it but never writes it. Remove `TREQ_REMOTE_SPRITES_STUB=1` if `scripts/service-qa/up.sh` put it there, and make sure these lines are present:

   ```bash
   TREQ_SPRITES_API_TOKEN=<test-org token>
   # TREQ_SPRITES_API_URL=https://api.sprites.dev   # optional, this is the default
   # The functions' own SUPABASE_URL is a Docker-internal address, so point the relay at one this machine can dial.
   REMOTE_SSH_RELAY_URL=http://127.0.0.1:54321/functions/v1/remote-ssh-relay
   # Local-only CA pair, the same one service-qa uses.
   REMOTE_SSH_CA_ED25519_SEED_BASE64=U7QP4DsQ6pPuNz9nyuniR6tEsHqTOvulJhpWElYm07Y=
   REMOTE_SSH_CA_ED25519_PUBLIC_KEY_BASE64=uL88vOSTAX5b7HkQyciJmsjAKWT4aJVnukznoRrgDic=
   ```

2. Run the script from the repository root:

   ```bash
   scripts/sprites-e2e-local.sh
   ```

The script:

- Checks the `.env`: the token, relay URL and CA must be set, and the stub must be off. If anything is wrong it stops and names the line to fix.
- Restarts the stack with `supabase start` so the functions pick up the `.env`, then runs `supabase db reset --local`. **This wipes your local database.**
- Reads the URL and keys from `supabase status -o env` and runs the suite.

To re-run against the same stack without restarting or resetting it:

```bash
SKIP_STACK=1 scripts/sprites-e2e-local.sh
```

`scripts/service-qa/up.sh` overwrites `supabase/functions/.env` with its stub setup. After running it, redo step 1.

## Option B: the hosted Supabase test project (what CI uses)

Use this when the test project's Edge Functions already have `TREQ_SPRITES_API_TOKEN` and `TREQ_SPRITES_API_URL` set to the test organization. See `remote_e2e_README.md`.

```bash
export TREQ_REMOTE_E2E=1 TREQ_REMOTE_E2E_NATIVE=1
export SUPABASE_TEST_URL=https://<test-project>.supabase.co
export SUPABASE_TEST_ANON_KEY=<anon key>
export SUPABASE_TEST_SERVICE_ROLE_KEY=<service role key>

cargo test --manifest-path src-tauri/Cargo.toml --test remote_e2e_native -- --ignored --nocapture --test-threads=1
```

Never point `SUPABASE_TEST_*` at production. The suite creates users and grants them Pro through the billing RPCs.

## Reading the result

A pass ends with `test native_certificate_auth_two_repos_mutations_pty_reconnect_and_reprovision_trust ... ok`. A line like `SKIP: TREQ_REMOTE_E2E_NATIVE=1 not set` or `missing native live credentials` followed by `ok` means nothing ran. A skipped run is not a pass. Without `--ignored`, cargo reports the test as `ignored`.

## When it fails

- **`ensure failed: … pro_required`**: the Pro grant did not take. Check that the billing migrations are applied (`supabase db reset --local`, or `supabase db push` on the test project).
- **Stuck below `ready`, then a timeout**: provisioning failed at Sprites. Look at the Edge Function logs (`supabase functions logs` locally, or the dashboard). Each call prints a `correlation_id` you can search for.
- **Relay or SSH errors on the first command**: in option A, check that `REMOTE_SSH_RELAY_URL` is reachable from your machine. If you edited the `.env` after the stack started, restart it (run without `SKIP_STACK`).
- **The run was killed, so no cleanup ran**: the Sprite is named `dev-treq-<user id>`. Delete it in the Sprites dashboard or CLI. Against the hosted test project, `deno run --allow-net --allow-env scripts/remote-e2e-cleanup.ts --dry-run` lists what is left over.

## CI

`.github/workflows/remote-e2e.yml` runs this suite when started by hand with the `native` input checked. It uses the same `SUPABASE_TEST_*` secrets as the other live suites.
