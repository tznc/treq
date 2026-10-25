---
name: service-qa
description: >-
  Verify treq's Supabase contract (Auth, RPCs/RLS, Edge Functions) with real
  calls against the local Supabase CLI stack, never a mocked client. Use on
  /service-qa, when asked to QA auth or Supabase flows, and proactively after
  changing supabase/** or app code that calls Supabase, before reporting done.
---

# Service QA (Supabase CLI integration checks)

## When to use

- User invokes `/service-qa`, optionally naming a flow ("service-qa desktop
  token exchange", "service-qa webhook HMAC").
- Proactively, immediately after an Edit/Write/MultiEdit that changes the
  Supabase service contract or the app code that calls it — don't wait to be
  asked. If you see `additionalContext` from `post-edit-service-qa.sh` naming a
  changed file, that *is* the request.

## Relationship to the desktop UI tests

| Check | Proves | Stack |
|---|---|---|
| Desktop UI tests (Rust, `src-tauri/src/gpui_shell/`) | UI and interaction | real jj, GPUI test windows; Supabase stubbed |
| `/service-qa` | Auth, RLS, RPCs, Edge Functions | real local Supabase CLI; **no** mocked supabase-js |

Do not substitute one for the other. A green UI test with a stubbed Supabase
does not prove the migration or Edge Function works. A green
service-qa RPC alone does not prove the desktop button wiring is correct — when
the user asks for screenshots of the UI talking to Supabase, capture via the
screenshot harness against `http://127.0.0.1:54321` (email/password session on
the app singleton; for a Pro user, post signed Stripe events with
`scripts/service-qa/stripe.ts` rather than stubbing the gate). Never
paste terminal logs or outcome HTML as stand-ins for those UI captures.

Pure Edge Function unit tests under `test/` stay in `npm test`. service-qa covers the **I/O boundary**: Postgres, Auth Admin API,
PostgREST, and `functions/v1/*`.

## Delegate grunt work and discovery to cheaper subagents

Most of the cost of a service-qa run is search and mechanical legwork, not
judgement. Push that to subagents on a cheaper model (`Agent` with
`model: "haiku"`, or `"sonnet"` when the answer needs some reasoning) and keep
the expensive context for the parts that actually need it. Run independent
delegations in parallel in a single message.

Good candidates — hand these off:

- **Finding prior art.** Which `scripts/service-qa/specs/` already covers this
  contract; which migration defines the RPC/table; which Edge Function the app
  invokes. Use `Explore`.
- **Tracing the app call site.** Which hook or page calls
  `supabase.rpc(...)` / `functions.invoke(...)` for the changed surface.
- **Mechanical sweeps.** Reading a batch of `<name>.json` manifests under
  `scripts/service-qa/.generated/`, diffing a new spec against the closest
  worked example.

Keep these yourself — they are the skill, not the legwork:

- Deciding what service contract to verify and what the `expectations` should
  claim.
- Writing the spec and driving the real local stack.
- **Step 5 verification.** Read the `.generated/<name>.json` outcomes yourself
  and confirm each claim against the test output / DB state. A subagent's
  "looks fine" is not that.
- The final report to the user.

## Ground rule: real local Supabase only, never mock the client

Every call inside a service-qa spec must hit the local CLI stack at
`http://127.0.0.1:54321` through the harness clients in
`scripts/service-qa/clients.ts` (anon JWT for user-scoped calls, service-role
for seeding / admin). Never `vi.mock("../../src/lib/supabase")`. Never point
at production (`*.supabase.co`).

If a flow needs a signed-in user, use **email/password only** via
`createTestUser` / `signInWithEmailPassword` in `scripts/service-qa/seed.ts`
(public `signUp` + `signInWithPassword`). Do not use OAuth. service-qa overrides
`package.json` → `featureFlags.emailSignup` to `true` in
`scripts/service-qa/features.ts` so this path stays available even when the
shipped product flag is off. Do not hard-code production user IDs or paste live
session tokens. Tear down every user and install row you seed in a try/finally.

GitHub App side effects are out of scope for early
specs unless you mock only the **outbound GitHub adapter** inside an Edge
Function test — never mock PostgREST or Auth themselves. Prefer specs that
assert DB rows, RPC return values, and HTTP status codes from Edge Functions.

## References

- `references/harness.md`: the local stack ports, health gate, clients, seed helpers, and `recordOutcome`. Read it when writing a new spec or when the stack or a helper misbehaves.

## Prerequisites

```bash
# One-time: CLI + Docker Desktop (or compatible engine) installed and running.
# https://supabase.com/docs/guides/local-development/cli/getting-started

npm run service-qa:up    # supabase start && db reset --local
# or: make start && make db.reset
```

Optional secrets for flows that need them (OAuth, GitHub App):

- `SUPABASE_AUTH_EXTERNAL_GOOGLE_SECRET` / `SUPABASE_AUTH_EXTERNAL_GITHUB_SECRET`
  (see `supabase/config.toml`)
- `GITHUB_APP_ID`, `GITHUB_APP_PRIVATE_KEY_BASE64`, `GITHUB_WEBHOOK_SECRET`
  for GitHub App / webhook specs

Email confirmations are **off** locally (`[auth.email] enable_confirmations =
false`), so password signup works without Inbucket. service-qa **overrides**
`featureFlags.emailSignup` to `true` in `scripts/service-qa/features.ts` and
creates users only via Auth email/password (`signUp` / `signInWithPassword`) —
never OAuth.

`public.subscriptions` and `has_pro` read `billing_subscriptions`, which only
the `stripe-webhook` function writes (`025_billing_entitlement.sql`). To make a
test user Pro, post signed `customer.subscription.*` and
`checkout.session.completed` events with `postStripeEvent` from
`scripts/service-qa/stripe.ts` (see `specs/stripe-webhook.spec.ts`). Local
signing uses `STRIPE_WEBHOOK_SECRET` from `supabase/functions/.env`.

## Steps

1. **Identify the contract to verify.** From the user's ask, or from the
   changed file(s) named in the hook's `additionalContext`, work out which
   Auth/RPC/Edge path changed. Search `scripts/service-qa/specs/` and the
   matching migration or `supabase/functions/<name>/` for prior art. Delegate
   that search to an `Explore` subagent; do the "which contract matters" call
   yourself.

2. **Write or extend a spec** under
   `scripts/service-qa/specs/<slug>.spec.ts`. One spec per contract. Auth must
   use email/password (`createTestUser` / `signInWithEmailPassword`) — the service-qa feature override keeps
   `emailSignup` on. If an existing spec already covers this flow, add steps
   to it rather than duplicating seed setup. Shape:

   ```ts
   import { it, expect } from "vitest";
   import { getAnonClient, getServiceClient } from "../clients";
   import { createTestUser, linkGithubRepo } from "../seed";
   import { recordOutcome } from "../record";

   it("verifies <the contract>", async () => {
     const admin = getServiceClient();
     const { user, password, email } = await createTestUser(admin);

     const anon = getAnonClient();
     const { data: signIn, error: signInError } =
       await anon.auth.signInWithPassword({ email, password });
     expect(signInError).toBeNull();
     expect(signIn.session).toBeTruthy();

     // Real service assertions -- prove the contract BEFORE recording.
     const { data, error } = await anon.rpc("some_rpc", { /* ... */ });
     expect(error).toBeNull();
     expect(data).toEqual(/* ... */);

     await recordOutcome("<slug>-01-<what>", {
       expectations: [
         "Plain-English claim about the service outcome a reviewer should confirm.",
         "A second claim, if needed.",
       ],
       details: { userId: user.id, data },
     });
   }, 60000);
   ```

   `scripts/service-qa/specs/desktop-token-exchange.spec.ts` is the worked
   example for Auth + Edge Function. `organizations.spec.ts` is the worked
   example for exercising RPCs as a signed-in user. Copy whichever shape fits.

   Give every outcome a numbered, descriptive `name` —
   `<slug>-<NN>-<what-it-shows>` — that string becomes the JSON filename.

3. **`expectations` are for the service outcome checklist, not a substitute
   for `expect`.** `recordOutcome` requires a non-empty `expectations:
   string[]` — plain-English claims about what the run proved (status codes,
   token single-use, RPC return value, RLS denial). These are written to
   `<name>.json` so step 5 has a concrete checklist. Keep real Vitest
   assertions in the spec body too.

   **At most 3 expectations per outcome.** If you
   need more, split into a second `recordOutcome` with its own `name`.

4. **Run it.**
   - First run in a session, or after touching migrations / `config.toml`:
     `npm run service-qa:full` (start + reset + all specs).
   - Stack already up, iterating on one spec:
     `npx vitest run --config vitest.service-qa.config.ts scripts/service-qa/specs/<slug>.spec.ts`
   - After a migration-only change with stack already running:
     `make db.reset && npm run service-qa`

5. **Verify each outcome against its expectations before saying the task is
   done.** For every `recordOutcome`: read
   `scripts/service-qa/.generated/<name>.json` and confirm or refute each
   claim against the Vitest output and, when relevant, a follow-up query via
   the service-role client or Studio (`http://127.0.0.1:54323`). A spec whose
   `expect` calls passed can still leave the wrong row shape or a half-applied
   migration — the checklist is what forces a second look.

6. **Show the result.** Report which contracts passed, quote any expectation
   that did not hold, and name the spec file(s) added or extended. Do not dump
   full JSON unless the user asks.

7. **Lint, format, and typecheck the whole change, last.** Only after the
   contract is verified:
   - `npm run format`
   - `npm run lint`
   - `npm run check`

   Fix everything these flag — including in any spec you added in step 2.

## Keep specs around

`scripts/service-qa/specs/` is a growing service-contract library, not a scratch
directory. Don't delete a spec after using it — if a later change touches the
same contract, extend its steps instead of writing a near-duplicate file.

## What not to do

- Do not mock `@supabase/supabase-js`.
- Do not hit production Supabase.
- Do not put business logic in the harness — only health, clients, seed, and
  outcome recording.
- Do not start the full desktop UI inside service-qa; the desktop UI tests
  cover it.
- Do not skip the failing step: a green test written against a mock is a false
  test for this skill.
