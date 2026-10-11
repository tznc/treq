# Testing the mobile app against real Sprites

`tests/sprites_e2e.rs` runs the mobile app's managed-instance flow against real Fly Sprites. It uses the app's own Rust control-plane client and treq's SSH transport, the same code the phone runs. The only difference is the device key: the phone keeps it in the OS keystore, the test keeps it in a temporary file.

## What one run does

1. Creates an e2e-tagged test user, grants it Pro (`ensure`/`wake` answer 402 without it) and signs in.
2. Checks that `status` returns no instance, then calls `ensure`, which provisions a real Sprite. It polls `status` until the instance is `ready` (up to 10 minutes).
3. Registers the device key with comment `treq-mobile-device` and idempotency key `register:<fingerprint>`, as `managed::issue` does. It replays the registration and checks that it gets the same key back.
4. Issues a certificate, checks that the endpoint uses the relay transport and pins host keys, then runs `InitRepo`, `InspectRepository`, `CreateWorkspace` and `ListWorkspaces` over the relay.
5. Renews the certificate (`renewal: true`), checks that the endpoint id did not change, and runs a read on the same connection pool.
6. Calls `wake`, waits for `ready`, reconnects on a fresh pool and checks that the workspace is still there.
7. Revokes the device key. It checks that the renewal is refused and that `managed::classify_renewal_error` maps the refusal to `KeyRevoked`, then calls `report_cutoff`.
8. On exit, pass or panic, it deletes the instance (and so the Sprite), the billing rows and the user.

Expect a run to take about 3 to 10 minutes. Most of that is provisioning.

## Prerequisites

- **A Sprites token for a dedicated test organization.** Sprites tokens are organization-scoped, so never use the production token. Create the token at sprites.dev under the test organization.
- Rust stable. On Linux, also install the system packages from the `unit-tests` job in `.github/workflows/mobile-gpui.yml`. On macOS no extra packages are needed.
- For option A: Docker and the [Supabase CLI](https://supabase.com/docs/guides/local-development/cli/getting-started).

The first build of the mobile crate compiles GPUI and takes a while. Later builds are incremental.

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

2. Run the script:

   ```bash
   cd treq
   mobile-gpui/scripts/sprites-e2e-local.sh
   ```

The script:

- Checks the `.env`: the token, relay URL and CA must be set, and the stub must be off. If anything is wrong it stops and names the line to fix.
- Restarts the stack with `supabase start` so the functions pick up the `.env`, then runs `supabase db reset --local`. **This wipes your local database.**
- Reads the URL and keys from `supabase status -o env` and runs the suite.

To re-run against the same stack without restarting or resetting it:

```bash
SKIP_STACK=1 mobile-gpui/scripts/sprites-e2e-local.sh
```

Note that `scripts/service-qa/up.sh` overwrites `supabase/functions/.env` with its stub setup. After running it, redo step 1.

## Option B: the hosted Supabase test project (what CI uses)

Use this when the test project's Edge Functions already have `TREQ_SPRITES_API_TOKEN` and `TREQ_SPRITES_API_URL` set to the test organization. See `src-tauri/tests/remote_e2e_README.md`.

```bash
cd treq
mkdir -p src/dist && echo '<!doctype html>' > src/dist/index.html   # once

export TREQ_REMOTE_E2E=1 TREQ_REMOTE_E2E_MOBILE=1
export SUPABASE_TEST_URL=https://<test-project>.supabase.co
export SUPABASE_TEST_ANON_KEY=<anon key>
export SUPABASE_TEST_SERVICE_ROLE_KEY=<service role key>

cd mobile-gpui
cargo test --test sprites_e2e -- --ignored --nocapture --test-threads=1
```

Never point `SUPABASE_TEST_*` at production. The suite creates users and grants them Pro through the billing RPCs.

## Expected output

```
[mobile-sprites-e2e] created Pro test user 6f1c…
[mobile-sprites-e2e] ensure (desktop's part): provisioning a Sprite
[mobile-sprites-e2e] instance 3b9e… ready after 94.2s
[mobile-sprites-e2e] device key a71d… registered
[mobile-sprites-e2e] running typed commands over the relay
[mobile-sprites-e2e] renewing the certificate
[mobile-sprites-e2e] waking the instance
[mobile-sprites-e2e] revoking the device key
[mobile-sprites-e2e] passed in 212.8s
[mobile-sprites-e2e] cleanup: delete instance -> Ok(200)
[mobile-sprites-e2e] cleanup: delete user -> Ok(200)
test mobile_managed_flow_against_real_sprites ... ok
```

A line like `SKIP: TREQ_REMOTE_E2E_MOBILE=1 not set` followed by `ok` means nothing ran. A skipped run is not a pass. Without `--ignored`, cargo reports the test as `ignored`.

## When it fails

- **`ensure: 402 … pro_required`**: the Pro grant did not take. Check that the billing migrations are applied (`supabase db reset --local`, or `supabase db push` on the test project).
- **Stuck below `ready`, then a timeout**: provisioning failed at Sprites. Look at the Edge Function logs (`supabase functions logs` locally, or the dashboard). Each failing call prints a `correlation_id` you can search for.
- **Relay or SSH errors on the first command**: in option A, check that `REMOTE_SSH_RELAY_URL` in `supabase/functions/.env` is reachable from your machine. If you edited the `.env` after the stack started, restart it (run without `SKIP_STACK`).
- **The run was killed, so no cleanup ran**: the Sprite is named `dev-treq-<user id>`. Delete it in the Sprites dashboard or CLI. Against the hosted test project, `deno run --allow-net --allow-env scripts/remote-e2e-cleanup.ts --dry-run` lists what is left over.

## CI

`.github/workflows/remote-e2e.yml` runs this suite when it is started by hand with the `mobile` input checked. It uses the same `SUPABASE_TEST_*` secrets as the other live suites.
