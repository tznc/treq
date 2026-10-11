# Remote Development acceptance traceability

This document maps the ship-blocking criteria in `prds/remote-development.md` to implementation tests.

The PRD intentionally defines user-visible shipping behavior rather than turning transport internals, provider quotas, certificate scheduling, audit plumbing, or test-resource cleanup into standalone product criteria.

## Test suites

Existing remote test suites continue to cover transport, control-plane, trust, lifecycle, mutation-safety, terminal, and UI behavior. Real-provider suites remain credential-gated and a skipped real-provider suite is not considered an acceptance pass.

## Acceptance criteria mapping

| # | Criterion | Existing evidence | Remaining acceptance gap |
|---|---|---|---|
| 1 | Managed setup works end-to-end | Managed provisioning/readiness tests, setup UI tests, native remote e2e harness, repository open/init commands | Run the complete user flow against the supported managed test environment, including returning to an existing repository |
| 2 | User-managed SSH works end-to-end | Host trust UI/unit tests, SSH transport tests, explicit endpoint registration structures, real-sshd integration harness | Full UI-to-real-host acceptance path |
| 3 | Remote repositories use the normal Treq workspace/review UI | Remote review refresh tests, query-key isolation, remote repository restore integration coverage | Full remote rendering of workspaces, diffs, commits, context, and conflicts |
| 4 | Core workspace mutations work remotely | Typed command construction, allow-list dispatch, structured error mapping, mutation verification recipes | End-to-end coverage of the complete set of mutations required by the normal Treq workflow |
| 5 | Agents work in remote workspaces | Agent remote command support and remote PTY/command infrastructure | End-to-end start/interact/reattach/stop of a coding agent on a provisioned remote environment |
| 6 | Interactive terminals work in remote workspaces | PTY cwd/I/O/resize/close tests, real-sshd PTY harness, native remote PTY test | Product-level detach/reattach flow across disconnect/app navigation |
| 7 | Reconnect is safe | Verify-before-retry transport tests and ambiguity handling | Acceptance coverage through a real mid-mutation network interruption |
| 8 | Out-of-band remote changes become visible | Change-marker refresh tests and direct-operation marker coverage | Product-level acceptance covering changes made from another client/agent while review UI is open |
| 9 | Trust failures fail closed | Host-key rejection, certificate cutoff/expiry/revocation, real-sshd trust tests | Run credential-gated trust paths in the supported acceptance environment |
| 10 | Normal managed lifecycle recovery works | Wake/suspend lifecycle tests, status banner tests, provider wake harness | Full suspend/wake/reconnect user flow against the supported managed provider |

## Supporting engineering coverage

The repository also contains tests for implementation requirements such as connection pooling, credential renewal, provider idempotency, resource configuration, audit redaction, observability, generation changes, and safe test-resource cleanup.

Those remain valuable engineering tests, but their existence does not add product acceptance criteria beyond the ten behaviors above.

## Real-environment testing

Credential-gated Sprites/Supabase/SSH suites must continue to report skipped runs honestly. A harness that compiles and skips because credentials are absent is not evidence that the corresponding acceptance criterion passed.

Operational setup, environment variables, cleanup safety gates, and provider-specific run instructions belong with the test harness (below) rather than in the product PRD.

## Running the real-provider suites

The managed provider is Fly Sprites. The suites test what Sprites actually offers:

- A Sprite has no region or machine size setting. The platform sizes it, so no test asserts on either.
- A Sprite has no suspend call. It pauses by itself about 30 seconds after the last activity (`warm`, then `cold` later). The lifecycle tests stop all activity and wait, with a bound, for the pause.
- Reading a Sprite's metadata does not wake it. Wake has to do work on the Sprite, so the lifecycle tests check that the Sprite reports `running` after wake. A wake that does nothing fails them.
- Repair (`reprovision`) keeps the same Sprite and its files, so the generation and the pinned host keys stay the same.

| Suite | What it covers | Command |
|---|---|---|
| `remote_e2e.rs` | Sprites adapter against the real API: idempotent create, readiness, exec reachability, idle pause then wake and reconnect with files intact, in-place repair, delete and inventory check | `TREQ_REMOTE_E2E=1 cargo test --test remote_e2e -- --ignored --test-threads=1` |
| `remote_e2e_native.rs` | The managed flow desktop and mobile share, over native certificate SSH through the relay: status, ensure, key registration and replay, certificate issue and renewal, two repositories, mutations, PTY, reconnect, idle pause then wake, repair with host trust carried over, key-revoked cutoff. Local run against real Sprites: [`SPRITES_E2E.md`](SPRITES_E2E.md) | add `TREQ_REMOTE_E2E_NATIVE=1`, `--test remote_e2e_native` |
| `supabase/functions/tests/remote_e2e.test.ts` | Deployed Edge Functions on the test project: provisioning, trust, wake, repair, audit, delete | `deno test --allow-net --allow-env supabase/functions/tests/remote_e2e.test.ts` (add `TREQ_REMOTE_E2E_SOAK=1` for the hours-long cold-Sprite soak) |

`.github/workflows/remote-e2e.yml` runs the adapter and Edge suites nightly and on demand, and skips cleanly when the secrets below are absent. The native suite and the soak run only on demand. A final step sweeps leftover e2e resources whether or not the tests passed. `.github/workflows/remote-e2e-cleanup.yml` is the daily orphan scan.

### Environment variables and repository secrets

Every provider credential here must belong to a dedicated Sprites test organization. Sprites tokens are organization-scoped, so a production token would let tests and cleanup touch real user Sprites.

| Variable (also the secret name) | Used by | Replaces |
|---|---|---|
| `TREQ_REMOTE_E2E` = `1` | all live suites; the workflow gate | unchanged |
| `TREQ_SPRITES_TEST_API_TOKEN` | `remote_e2e.rs` | `SPRITES_TEST_API_TOKEN`, `FLY_TEST_API_TOKEN` |
| `TREQ_SPRITES_TEST_API_URL` (optional, default `https://api.sprites.dev`) | `remote_e2e.rs` | `SPRITES_TEST_API_URL`, `FLY_TEST_API_BASE_URL` |
| none | | `FLY_TEST_APP_NAME` (Sprites have no app) |
| `TREQ_SPRITES_E2E_CLEANUP_API_TOKEN` | `scripts/remote-e2e-cleanup.ts` | `SPRITES_E2E_CLEANUP_API_TOKEN`, `FLY_E2E_CLEANUP_API_TOKEN` |
| `TREQ_SPRITES_E2E_CLEANUP_API_URL` (optional, default `https://api.sprites.dev`) | `scripts/remote-e2e-cleanup.ts` | `SPRITES_E2E_CLEANUP_API_URL`, `FLY_E2E_CLEANUP_API_BASE_URL` |
| none | | `FLY_E2E_CLEANUP_APP_NAME` (Sprites have no app) |
| `SUPABASE_TEST_URL`, `SUPABASE_TEST_ANON_KEY`, `SUPABASE_TEST_SERVICE_ROLE_KEY`, `REMOTE_ADMIN_API_KEY_TEST` | native and Edge suites, cleanup | unchanged |
| `TREQ_REMOTE_E2E_CLEANUP_TARGET` = `dedicated-test` | cleanup apply mode | unchanged |

Local-only tuning, not secrets:

- `TREQ_REMOTE_E2E_MAX_CONCURRENCY`: live Sprites `remote_e2e.rs` may hold at once (default 2).
- `TREQ_REMOTE_E2E_IDLE_PAUSE_TIMEOUT_SECS`: how long `remote_e2e.rs` waits for a Sprite to pause (default 600).
- `TREQ_REMOTE_E2E_IDLE_PAUSE_WAIT_SECS`: how long `remote_e2e_native.rs` idles before it wakes the VM (default 90).

The Edge Functions on the Supabase test project read their own `TREQ_SPRITES_API_URL` and `TREQ_SPRITES_API_TOKEN` function secrets. Set those to the same test organization.

The native and Edge suites grant their test users Pro through the billing RPCs (`billing_record_checkout_completed`, `billing_record_subscription_event`), because `ensure`, `wake` and `reprovision` answer 402 without it.

Sprites created by `remote_e2e.rs` are named `dev-treq-treq-e2e-<uuid>`. Sprites created through the Edge Functions are named `dev-treq-<user id>` for an e2e-tagged test user. The cleanup script deletes only those two shapes and refuses anything else.
