# Billing chaos test report (2026-10-31)

**Stack:** a local Supabase CLI stack, with all migrations applied through 037. Webhooks were signed with the local `whsec_service_qa_local` secret. The Edge runtime could not boot `npm:` imports behind the sandbox proxy, so `SERVICE_QA_NO_EDGE=1` ran `stripe-webhook/lib.ts` in-process. Its writes still went through PostgREST to Postgres, using the real security-definer functions.

**Probe spec:** `scripts/service-qa/specs/billing-chaos.spec.ts` (9 cases, all green). Outcome files are in `scripts/service-qa/.generated/billing-chaos-*.json`.

**Baseline:** the existing `stripe-webhook` and `billing-*` specs passed (6 files, 7 tests).

## What held up (no action)

| Probe | Result |
|---|---|
| Six bad webhook signatures: unsigned, wrong secret, zeroed HMAC, v0-only, 10 minutes old, 10 minutes in the future | All returned 400 and wrote nothing |
| One event id delivered 8 times concurrently | 1 `applied`, 7 `duplicate`, 1 row |
| `subscription.created` arriving before `checkout.session.completed` | Pro is granted once the checkout event lands |
| An older `.updated` arriving after `.deleted` | Ignored as `stale`; Pro is not restored |
| Two checkouts at the same time naming different owners for one customer | One `applied`, one `owner_conflict` |
| RLS bypass attempts | Client INSERT on `billing_subscriptions` and `billing_customers` refused (42501). UPDATE matched 0 rows. Reading `billing_events` and calling `billing_record_*`, `billing_attach_customer` or `organization_has_team` refused (42501). `has_pro(other user)` returns false. |

## Findings, ranked by severity

### 1. High: a double checkout charges the user twice, and nothing detects it
- **Repro (spec case 05):**
  1. Open `billing-checkout` in two tabs before the first webhook lands. `hasPro()` is still false, so both requests get a session.
  2. Complete both.
  3. Two `customer.subscription.created` events both return `applied`, leaving 2 active Pro rows for one customer.
- **Impact:** the user pays twice. Both sessions can also include the 14-day trial, because `trial_used_at` is only set once a webhook arrives.
- **Options:**
  - **A.** In `billing-checkout`, before creating a session, list the customer's open checkout sessions and active subscriptions in Stripe. Expire the open sessions, or return 409.
  - **B.** Pass a Stripe idempotency key on session creation, derived from the owner, plan and a short time bucket.
  - **C.** When the webhook sees a second live subscription of the same plan for one owner, record an alert, cancel the newer subscription with a refund, and return `duplicate_subscription`.
- **Recommendation:** A as the main fix, plus C as a backstop.

### 2. High: Pro never expires if the `.deleted` webhook is lost (entitlement drift)
- **Repro (spec case 06):** a row with status `active` and `current_period_end` 60 days in the past still returns `has_pro = true`. The desktop shows "Active · Renews on 2026-09-01".
- **Impact:**
  - If Stripe exhausts its retries or the endpoint is disabled, a churned user keeps Pro indefinitely.
  - `past_due` counts as entitled with no time limit.
  - The remote-workspace lapse sweep never fires for these users.
- **Options:**
  - **A.** In `user_has_pro` and `organization_has_team`, add `current_period_end > now() - <grace>` (for example 3 days) for active and past_due rows.
  - **B.** Add a scheduled reconcile job that re-fetches stale subscriptions from Stripe.
  - **C.** Both.
- **Recommendation:** C. A is the cheap, safe gate; B corrects data that has already drifted.

### 3. Medium: same-second events are applied in delivery order, so the plan can show the wrong state
- **Repro (spec case 04):** two `.updated` events share `event.created`: "schedule cancel" first, then "reactivate". If they are delivered in reverse order, the final row has `cancel_at_period_end=true`, and the desktop shows "Canceling · Pro until …" for a subscription that will actually renew.
- **Cause:** the guard is `stripe_event_created <= excluded.stripe_event_created`, and Stripe's `created` only has 1-second resolution.
- **Options:**
  - **A.** When `created` ties, fetch the subscription from Stripe and write its current state.
  - **B.** Store a tie-breaker (for example the object's `latest_invoice` or the event's `pending_webhooks`); this is fragile.
  - **C.** Use thin events: always re-fetch the subscription from Stripe before writing.
- **Recommendation:** A, the smallest change that is correct.

### 4. Medium: moving a Pro subscription to the Team price removes Pro but keeps billing
- **Repro (spec case 07):** a user-owned subscription updated to `treq_team_monthly` is written as `plan='team'` and `owner_type='user'`. `has_pro` becomes false, while Stripe bills $199.
- **Reachable if:** the Stripe customer-portal configuration allows switching price. That configuration lives in the Stripe dashboard, and I couldn't check it from here.
- **Options:**
  - **A.** Restrict the portal configuration to the same product. This is an ops-only change.
  - **B.** In the webhook, ignore or alert when a plan doesn't match its owner type (`user`+`team` or `org`+`pro`), and return `owner_plan_mismatch`.
  - **C.** Treat `user`+`team` as Pro.
- **Recommendation:** A and B together.

### 5. Low: the docs say the lapse grace period is 30 days, the code uses 7
- `027_billing_enforcement.sql` comments say "After 30 days … deletes". `031_retention_windows.sql` and `lapse-sweep.ts` (`LAPSE_GRACE_DAYS = 7`) say 7. Users and support may quote the wrong number.
- **Options:**
  - **A.** Fix the comments.
  - **B.** Also surface the deletion date in the desktop and web UI.
  - **C.** Leave it.
- **Recommendation:** B.

### 6. Low: the desktop says "Renews on" for a past_due subscription
- In `src-tauri/src/gpui_shell/screen/settings/account.rs:344`, `period_label("past_due")` returns "Renews on". A failed payment reads as a healthy renewal, and there is no call to update the card.
- **Options:**
  - **A.** Show "Payment failed · retrying until …".
  - **B.** Option A plus a "Update payment method" button that opens the billing portal.
  - **C.** Hide the date when the subscription is past due.
- **Recommendation:** B.

## Not covered, and why
- **Usage metering, credits and over-limit:** there is none. Managed sandboxes have fixed quotas (5 GB disk, 1 vCPU, 2 GB RAM, enforced by check constraints), one per Pro user. The quota-exhaustion and metering-gap scenarios don't apply. The nearest equivalent, lapse after Pro ends, is already covered by the existing `billing-enforcement-remote` spec.
- **Card decline, proration and real checkout:** these need live Stripe test-mode keys. Their effect on our side reduces to `past_due`/`incomplete` subscription events (findings 2 and 6) and seat-quantity changes.
- **Desktop screenshots (`app-qa`):** not captured. Building the GPUI app in this container wasn't attempted. Findings 2, 3 and 6 are shown through the `public.subscriptions` rows the desktop reads, which are in the outcome JSON. I can run `app-qa` as a follow-up.
- **Desktop entitlement refresh:** it rereads on window focus and keeps the last value if the read fails. That's acceptable.
