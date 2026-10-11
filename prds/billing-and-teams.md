# Billing and Teams

## Status

Draft.

This document defines how Treq sells Pro and Team, how the service decides who is entitled to the cloud features, and how organizations share a GitHub App installation. The plan decisions come from [Marketing](./marketing.md#approved-decisions). This document covers the mechanism.

## Goals

- A signed-in user can start a Pro trial or buy Team from the web dashboard, and manage or cancel it without contacting anyone.
- The service decides entitlement from records keyed by user or organization ID. An email address never decides it.
- Every cloud feature checks entitlement on the server. A modified build of the open-source app cannot turn a cloud feature on.
- An organization owns its GitHub App installation, its merge queue configuration, and its Team subscription.

## Non-goals

- Usage-based billing, add-ons, or more than two paid plans.
- Per-seat billing for Team. Team is a flat price.
- Invoicing outside Stripe, purchase orders, or annual plans.
- Billing inside the desktop or mobile app. Purchases happen on treq.dev.

## Plans

| Plan | Price | Entitlement |
| --- | --- | --- |
| Free | US$0 | No cloud features. |
| Pro | US$15 per user per month, 14-day trial with a card | `pro` for the subscribing user. |
| Team | US$199 per month for 5 members, plus US$8 per month for each member beyond 5 | `pro` for every member of the organization. |

The trial applies to Pro only. A user gets one Pro trial per Stripe customer, recorded when the first trial starts.

The cloud features are the GitHub App and its merge queue, managed cloud workspaces, mobile access, and Linear OAuth. GitHub through the local `gh` CLI and Linear through a personal API key stay free.

## Entitlement

A user is entitled to `pro` when either of these holds:

1. The user owns a Pro subscription whose status is `trialing`, `active`, or `past_due`.
2. The user is a member of an organization that owns a Team subscription with one of those statuses.

`past_due` keeps the entitlement while Stripe retries the payment. Stripe moves the subscription to `canceled` or `unpaid` when retries run out, and the entitlement ends then. A subscription set to cancel at the period end stays entitled until that date, and the dashboard shows it as canceling.

One security-definer function, `public.has_pro(user_id uuid)`, answers the question. Every server-side check calls it. Nothing else reads subscription rows to decide access.

## Data model

Stripe stays the source of truth for payment state. The webhook copies the fields Treq needs into Postgres, so checks never call the Stripe API and never match on email.

| Table | Purpose | Written by |
| --- | --- | --- |
| `billing_customers` | Maps an owner (`user` or `organization`) to a Stripe customer ID. Records `trial_used_at`. An owner keeps its first customer, and a customer keeps its first owner. | Checkout function and webhook |
| `billing_subscriptions` | One row per Stripe subscription: owner, plan (`pro` or `team`), status, `current_period_end`, `cancel_at_period_end`, `trial_end`. | Webhook only |
| `billing_events` | Stripe event IDs already processed, so a replayed event changes nothing. | Webhook only |
| `organizations` | Name and owner. | Organization function |
| `organization_members` | User, organization, and role (`owner` or `member`). | Organization function |
| `organization_invites` | Email, hashed token, inviter, expiry, accepted time. | Organization function |

`github_app_installations` gains a nullable `organization_id`. Row-level security lets a user read an installation when they linked it or when they belong to its organization. The same rule extends to repositories and merge queue configuration.

`public.subscriptions` keeps its current columns (`plan`, `status`, `current_period_end`), so the desktop app needs no change to read it. Its source moves from the Stripe foreign data wrapper to `billing_subscriptions` and `has_pro`. A later migration removes the foreign data wrapper and its vault secret.

## Edge functions

| Function | Auth | Behavior |
| --- | --- | --- |
| `billing-checkout` | User JWT | Creates or reuses the owner's Stripe customer, then creates an embedded Checkout session in subscription mode. Prices come from the lookup keys `treq_pro_monthly` and `treq_team_monthly`. Team buys one unit per member. Pro sets `trial_period_days: 14` unless `trial_used_at` is set, and always collects a card. Team requires the caller to own the organization. Returns the session's client secret. |
| `billing-portal` | User JWT | Returns a Stripe customer portal URL for the caller's customer, or for an organization they own. |
| `stripe-webhook` | Stripe signature | Verifies `Stripe-Signature` with `STRIPE_WEBHOOK_SECRET`, skips events already in `billing_events`, and upserts `billing_subscriptions` on `customer.subscription.created`, `updated`, and `deleted`. `checkout.session.completed` confirms the customer mapping and sets `trial_used_at`. |
| `organizations` | User JWT | Creates an organization, invites by email, accepts an invite by token, promotes and demotes owners, removes a member, deletes the organization, and attaches a GitHub App installation the caller linked. After a join, leave or removal it sets the Team subscription's quantity to the member count. |

The Checkout session carries the owner type and ID in `metadata` and `client_reference_id`. The webhook reads the owner from the customer mapping, not from the email on the Stripe customer.

The webhook trusts the owner in session `metadata` only when `client_reference_id` matches it, because a Payment Link accepts `client_reference_id` from its URL. A mapping that conflicts with an existing one is logged and ignored. Stripe does not deliver events in order, so each subscription row stores the creation time of the event that last wrote it. An older event never overwrites a newer one, and a canceled subscription is never revived.

## Enforcement

Each check calls `has_pro` on the server. The desktop and web gates stay for display only.

| Feature | Where it is checked | When entitlement ends |
| --- | --- | --- |
| GitHub App install | `create-github-install-intent` and `complete-github-installation`. The App requests user authorization during installation, and the function confirms the installation appears in GitHub's `/user/installations` for that user before linking. | Existing installations stay linked. The merge queue stops. |
| Merge queue | `set_merge_queue_enabled` and the merge queue worker | The worker pauses the queue and comments once on each queued pull request. |
| Linear OAuth | `create-linear-oauth-intent` and `linear-proxy` | The proxy returns 402 and the app falls back to the personal API key. |
| Managed cloud workspace | `remote-instance` actions `ensure`, `wake`, and `reprovision`, plus the SSH relay and key install that would wake it | The instance stops. Its disk is kept for 30 days, then deleted. `status` and `delete` always work. |
| Mobile access | The same checks as cloud workspaces, since mobile reaches them through Remote Development | As above. |

For an organization's installation, the merge queue checks the organization's Team subscription. For a personal installation, it checks the linking user.

Sprites have no stop call. A Sprite pauses on its own about 30 seconds after its last activity, so the service stops it by refusing every action that would wake it. Its disk is still billed by the provider for those 30 days. An hourly scheduled job runs the lapse sweep, because the project has no in-database scheduler.

## Organizations and seats

The user who creates an organization becomes its owner. The owner buys Team, invites members by email, and removes them. Team has no member cap. Its price `treq_team_monthly` is graduated: the first 5 units carry the flat US$199 and each unit after that costs US$8. The subscription's quantity is the member count, set at checkout and updated with proration by the `organizations` function after every join, leave and removal. Pending invites are not billed. Account deletion does not update the quantity, so a seat freed that way is corrected at the next join, leave or removal.

An invite link holds a single-use token that expires after 7 days. The token sits in the URL fragment, so it never reaches analytics or server logs. Accepting it requires signing in with any account. The invite is matched by token, not by email, so a member can join with a different address.

Today the last user to link a GitHub organization's installation takes it over. With organizations, linking an installation that belongs to a GitHub organization asks which Treq organization owns it. Only an owner of that Treq organization can relink it afterwards.

Leaving or being removed from an organization ends that member's Team entitlement at once. A personal Pro subscription is unaffected.

An owner can promote a member to owner and demote another owner. The last owner cannot be demoted, removed, or leave. To hand an organization over, an owner promotes a member and then leaves. An owner can delete the organization once its Team subscription has ended. Deleting it detaches its installations.

When an account is deleted and it was the last owner, the longest-standing member becomes owner. If no members remain, the organization is deleted. Deleting an account does not cancel any Stripe subscription.

## Clients

Web dashboard:

- The Subscription tab opens embedded Checkout for Pro, shows trial and renewal dates, and links to the customer portal.
- A Team tab creates an organization, buys Team, lists members and pending invites, and shows members against the 5 included seats and any extra seats billed.
- The pricing page links its Pro and Team buttons to the matching Checkout.

Desktop app:

- Treats `plan = 'pro'` with status `trialing`, `active`, `past_due`, or `canceled` (the cancel-at-period-end case) as Pro.
- Refetches the subscription when the window gains focus and after the user returns from the dashboard.
- Saves refreshed Supabase tokens, so a restart does not sign the user out.

## Rollout

Each step ships on its own and leaves the product working.

1. Migration, `has_pro`, the rewritten `subscriptions` view, and `stripe-webhook`. Nobody can buy yet, so nothing changes for users.
2. `billing-checkout`, `billing-portal`, and the dashboard. The owner turns on `stripePayments` once the Stripe products, portal, webhook and secrets exist.
3. Server-side enforcement. This step waits for step 2, so no user loses a feature they cannot buy.
4. Organizations, Team checkout, and installation ownership.
5. Pricing page copy for Pro as the cloud bundle, the trial, and Team.

Stripe currently has no paying subscribers, because checkout never shipped. No backfill is needed.

## Testing

- pgTAP covers `has_pro` for every status, row-level security on the new tables, and organization access to installations.
- Webhook tests post signed fixture events and check idempotency and status transitions.
- `service-qa` runs Checkout in Stripe test mode against the local Supabase stack, from trial start through cancellation.
- A takeover spec checks that a forged installation ID is refused and the real owner keeps the installation.
- pgTAP covers owner promotion and demotion, last-owner protection, refused deletion while Team is active, and account deletion.

## Open decisions

| ID | Decision | Default in this document |
| --- | --- | --- |
| B01 | Does Team get a trial? | No. Only Pro has a trial. |
| B02 | How long is a lapsed cloud workspace kept? | 30 days, stopped, then deleted. |
| B03 | Should a member who leaves a Team keep `pro` until the period ends? | No. Entitlement ends at once. |
| B04 | Should deleting an account or organization cancel its Stripe subscription? | No. The subscription keeps billing until canceled in the portal. |
| B05 | Can an owner delete an organization whose Team is set to cancel at the period end? | No. Deletion waits until the period ends. |
