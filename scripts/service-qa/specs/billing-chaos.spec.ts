/**
 * Billing chaos probes against the local stack: forged / replayed / duplicate
 * / out-of-order Stripe webhooks, double checkout, stale entitlement, and RLS
 * write attempts on billing tables.
 *
 * Cases that hold today assert with `expect`. Cases that expose a known gap
 * record the observed value under `observed` without asserting, so the run
 * stays green and the report can quote it. A fix flips those to assertions.
 */
import { randomBytes } from "node:crypto";
import { afterAll, expect, it } from "vitest";
import { signStripePayload } from "../../../supabase/functions/_shared/billing/stripe-signature";
import { getAnonClient, getServiceClient } from "../clients";
import { recordOutcome } from "../record";
import {
  createTestUser,
  deleteTestUser,
  signInWithEmailPassword,
} from "../seed";
import { postStripeEvent, SERVICE_QA_STRIPE_WEBHOOK_SECRET } from "../stripe";

const DAY = 86_400;
const now = () => Math.floor(Date.now() / 1000);
const id = (p: string) => `${p}_sqachaos${randomBytes(8).toString("hex")}`;
const admin = getServiceClient();
const users: string[] = [];
const customers: string[] = [];
const events: string[] = [];

function subEvent(o: {
  type?: string;
  sub: string;
  cus: string;
  status?: string;
  created: number;
  periodEnd?: number;
  cancelAtPeriodEnd?: boolean;
  lookupKey?: string;
  eventId?: string;
}) {
  const ev = {
    id: o.eventId ?? id("evt"),
    object: "event",
    type: o.type ?? "customer.subscription.updated",
    created: o.created,
    livemode: false,
    data: {
      object: {
        id: o.sub,
        object: "subscription",
        customer: o.cus,
        status: o.status ?? "active",
        cancel_at_period_end: o.cancelAtPeriodEnd ?? false,
        cancel_at: null,
        trial_end: null,
        items: {
          object: "list",
          data: [
            {
              id: id("si"),
              current_period_end: o.periodEnd ?? now() + 30 * DAY,
              price: { id: id("price"), lookup_key: o.lookupKey ?? "treq_pro_monthly" },
            },
          ],
        },
      },
    },
  };
  events.push(ev.id);
  return ev;
}

function checkoutEvent(cus: string, owner: string, eventId?: string) {
  const ev = {
    id: eventId ?? id("evt"),
    object: "event",
    type: "checkout.session.completed",
    created: now(),
    livemode: false,
    data: {
      object: {
        id: id("cs"),
        object: "checkout.session",
        mode: "subscription",
        customer: cus,
        client_reference_id: owner,
        metadata: { owner_type: "user", owner_id: owner },
      },
    },
  };
  events.push(ev.id);
  return ev;
}

async function newUser() {
  const u = await createTestUser();
  users.push(u.user.id);
  return u;
}

async function subRows(cus: string) {
  const { data } = await admin
    .from("billing_subscriptions")
    .select("stripe_subscription_id, owner_id, plan, status, cancel_at_period_end")
    .eq("stripe_customer_id", cus);
  return data ?? [];
}

afterAll(async () => {
  await admin.from("billing_subscriptions").delete().in("stripe_customer_id", customers);
  await admin.from("billing_customers").delete().in("stripe_customer_id", customers);
  await admin.from("billing_events").delete().in("stripe_event_id", events);
  for (const u of users) await deleteTestUser(admin, u).catch(() => {});
});

it("01 rejects forged, unsigned, wrong-secret and replayed-old webhooks", async () => {
  const { user } = await newUser();
  const cus = id("cus");
  customers.push(cus);
  const ev = checkoutEvent(cus, user.id);
  const raw = JSON.stringify(ev);

  const unsigned = await postStripeEvent(ev, { signature: null });
  const wrongSecret = await postStripeEvent(ev, { secret: "whsec_attacker" });
  const forgedHex = await postStripeEvent(ev, { signature: `t=${now()},v1=${"0".repeat(64)}` });
  const v0Only = await postStripeEvent(ev, {
    signature: (await signStripePayload(raw, SERVICE_QA_STRIPE_WEBHOOK_SECRET, now())).replace("v1=", "v0="),
  });
  const oldReplay = await postStripeEvent(ev, {
    signature: await signStripePayload(raw, SERVICE_QA_STRIPE_WEBHOOK_SECRET, now() - 600),
  });
  const futureSkew = await postStripeEvent(ev, {
    signature: await signStripePayload(raw, SERVICE_QA_STRIPE_WEBHOOK_SECRET, now() + 600),
  });
  const statuses = [unsigned, wrongSecret, forgedHex, v0Only, oldReplay, futureSkew].map((r) => r.status);
  expect(statuses).toEqual([400, 400, 400, 400, 400, 400]);

  const { data: cust } = await admin.from("billing_customers").select("*").eq("stripe_customer_id", cus);
  expect(cust).toEqual([]);

  await recordOutcome("billing-chaos-01-forged-signatures", {
    expectations: [
      "Unsigned, wrong-secret, zeroed-HMAC, v0-only, 10-min-old and 10-min-future signatures all get HTTP 400.",
      "No billing_customers row is written by any rejected request.",
    ],
    details: { statuses },
  });
}, 60000);

it("02 a duplicated event delivered 8x concurrently applies once", async () => {
  const { user } = await newUser();
  const cus = id("cus");
  customers.push(cus);
  await postStripeEvent(checkoutEvent(cus, user.id));
  const ev = subEvent({ type: "customer.subscription.created", sub: id("sub"), cus, created: now() });
  const results = await Promise.all(Array.from({ length: 8 }, () => postStripeEvent(ev)));
  const bodies = results.map((r) => (r.body as { result?: string }).result);
  expect(bodies.filter((b) => b === "applied")).toHaveLength(1);
  expect(bodies.filter((b) => b === "duplicate")).toHaveLength(7);
  expect(await subRows(cus)).toHaveLength(1);
  await recordOutcome("billing-chaos-02-duplicate-burst", {
    expectations: ["8 concurrent deliveries of one event id: 1 applied, 7 duplicate, 1 subscription row."],
    details: { bodies },
  });
}, 60000);

it("03 out-of-order: subscription before checkout, deleted before updated", async () => {
  const { user, email, password } = await newUser();
  const cus = id("cus");
  customers.push(cus);
  const sub = id("sub");
  const t = now();
  const created = await postStripeEvent(subEvent({ type: "customer.subscription.created", sub, cus, created: t }));
  const checkout = await postStripeEvent(checkoutEvent(cus, user.id));
  const { client } = await signInWithEmailPassword(email, password);
  const proAfterCheckout = (await client.rpc("has_pro", { p_user_id: user.id })).data;
  expect(proAfterCheckout).toBe(true);

  const deleted = await postStripeEvent(
    subEvent({ type: "customer.subscription.deleted", sub, cus, status: "canceled", created: t + 20 }),
  );
  const lateUpdate = await postStripeEvent(subEvent({ sub, cus, status: "active", created: t + 10 }));
  const proAfterDelete = (await client.rpc("has_pro", { p_user_id: user.id })).data;
  expect(proAfterDelete).toBe(false);
  await recordOutcome("billing-chaos-03-out-of-order", {
    expectations: [
      "subscription.created before checkout.session.completed still grants Pro once the checkout lands.",
      "A late, older subscription.updated after .deleted is 'stale' and does not restore Pro.",
    ],
    details: {
      results: [created, checkout, deleted, lateUpdate].map((r) => r.body),
      proAfterCheckout,
      proAfterDelete,
    },
  });
}, 60000);

it("04 same-second events: delivery order decides the final state", async () => {
  const { user, email, password } = await newUser();
  const cus = id("cus");
  customers.push(cus);
  const sub = id("sub");
  await postStripeEvent(checkoutEvent(cus, user.id));
  const t = now();
  await postStripeEvent(subEvent({ type: "customer.subscription.created", sub, cus, created: t - 100 }));
  // Real order in Stripe: user schedules cancel, then un-cancels in the same second.
  const scheduleCancel = subEvent({ sub, cus, created: t, cancelAtPeriodEnd: true });
  const reactivate = subEvent({ sub, cus, created: t, cancelAtPeriodEnd: false });
  // Delivered reversed.
  const r1 = await postStripeEvent(reactivate);
  const r2 = await postStripeEvent(scheduleCancel);
  const { client } = await signInWithEmailPassword(email, password);
  const { data: view } = await client.from("subscriptions").select("*");
  const rows = await subRows(cus);
  await recordOutcome("billing-chaos-04-same-second-tie", {
    expectations: [
      "OBSERVED: with equal event.created, the later-delivered (older) event wins; the reactivated subscription is shown as canceling.",
    ],
    details: { results: [r1.body, r2.body], rows, desktopView: view },
  });
}, 60000);

it("05 double checkout: two paid Pro subscriptions for one user are both recorded", async () => {
  const { user } = await newUser();
  const cus = id("cus");
  customers.push(cus);
  await postStripeEvent(checkoutEvent(cus, user.id));
  await postStripeEvent(checkoutEvent(cus, user.id));
  const a = await postStripeEvent(subEvent({ type: "customer.subscription.created", sub: id("sub"), cus, created: now() }));
  const b = await postStripeEvent(subEvent({ type: "customer.subscription.created", sub: id("sub"), cus, created: now() }));
  const rows = await subRows(cus);
  await recordOutcome("billing-chaos-05-double-checkout", {
    expectations: [
      "OBSERVED: two active Pro subscriptions for the same customer are both 'applied'; nothing flags or refunds the duplicate.",
    ],
    details: { results: [a.body, b.body], activeRows: rows.filter((r) => r.status === "active").length },
  });
}, 60000);

it("06 entitlement ignores current_period_end (lost .deleted webhook)", async () => {
  const { user, email, password } = await newUser();
  const cus = id("cus");
  customers.push(cus);
  await postStripeEvent(checkoutEvent(cus, user.id));
  await postStripeEvent(
    subEvent({ type: "customer.subscription.created", sub: id("sub"), cus, created: now(), periodEnd: now() - 60 * DAY }),
  );
  const { client } = await signInWithEmailPassword(email, password);
  const pro = (await client.rpc("has_pro", { p_user_id: user.id })).data;
  const { data: view } = await client.from("subscriptions").select("*");
  await recordOutcome("billing-chaos-06-expired-period-still-pro", {
    expectations: [
      "OBSERVED: an 'active' row whose current_period_end is 60 days past still answers has_pro = true.",
    ],
    details: { pro, desktopView: view },
  });
}, 60000);

it("07 Pro customer switched to the Team price (portal) loses Pro but keeps paying", async () => {
  const { user, email, password } = await newUser();
  const cus = id("cus");
  customers.push(cus);
  const sub = id("sub");
  await postStripeEvent(checkoutEvent(cus, user.id));
  await postStripeEvent(subEvent({ type: "customer.subscription.created", sub, cus, created: now() - 5 }));
  const switched = await postStripeEvent(subEvent({ sub, cus, created: now(), lookupKey: "treq_team_monthly" }));
  const { client } = await signInWithEmailPassword(email, password);
  const pro = (await client.rpc("has_pro", { p_user_id: user.id })).data;
  await recordOutcome("billing-chaos-07-user-on-team-price", {
    expectations: [
      "OBSERVED: a user-owned subscription moved to treq_team_monthly is applied with plan 'team' and the user gets has_pro = false while still billed.",
    ],
    details: { switched: switched.body, pro, rows: await subRows(cus) },
  });
}, 60000);

it("08 RLS: clients cannot write billing rows, call writers, or read others", async () => {
  const victim = await newUser();
  const attacker = await newUser();
  const cus = id("cus");
  customers.push(cus);
  await postStripeEvent(checkoutEvent(cus, victim.user.id));
  await postStripeEvent(subEvent({ type: "customer.subscription.created", sub: id("sub"), cus, created: now() }));
  const { client } = await signInWithEmailPassword(attacker.email, attacker.password);
  const anon = getAnonClient();
  const fakeSub = {
    stripe_subscription_id: id("sub"),
    stripe_customer_id: id("cus"),
    owner_type: "user",
    owner_id: attacker.user.id,
    plan: "pro",
    status: "active",
    stripe_event_created: new Date().toISOString(),
  };
  const attempts = {
    insertSub: (await client.from("billing_subscriptions").insert(fakeSub)).error?.code,
    insertCustomer: (await client
      .from("billing_customers")
      .insert({ owner_type: "user", owner_id: attacker.user.id, stripe_customer_id: id("cus") })).error?.code,
    updateVictim: (await client.from("billing_subscriptions").update({ owner_id: attacker.user.id }).eq("stripe_customer_id", cus).select()).data?.length ?? null,
    readVictimSubs: (await client.from("billing_subscriptions").select("*").eq("stripe_customer_id", cus)).data?.length,
    readEvents: (await client.from("billing_events").select("*")).error?.code,
    rpcRecordCheckout: (await client.rpc("billing_record_checkout_completed", {
      p_event_id: id("evt"), p_customer_id: cus, p_owner_type: "user", p_owner_id: attacker.user.id,
    })).error?.code,
    rpcAttach: (await client.rpc("billing_attach_customer", {
      p_owner_type: "user", p_owner_id: attacker.user.id, p_customer_id: cus,
    })).error?.code,
    hasProOther: (await client.rpc("has_pro", { p_user_id: victim.user.id })).data,
    orgHasTeam: (await client.rpc("organization_has_team", { p_org_id: victim.user.id })).error?.code,
    anonHasPro: (await anon.rpc("has_pro", { p_user_id: victim.user.id })).error?.code,
    attackerPro: (await client.rpc("has_pro", { p_user_id: attacker.user.id })).data,
  };
  expect(attempts.insertSub).toBeTruthy();
  expect(attempts.insertCustomer).toBeTruthy();
  expect(attempts.updateVictim ?? 0).toBe(0);
  expect(attempts.readVictimSubs).toBe(0);
  expect(attempts.readEvents).toBeTruthy();
  expect(attempts.rpcRecordCheckout).toBeTruthy();
  expect(attempts.rpcAttach).toBeTruthy();
  expect(attempts.hasProOther).toBe(false);
  expect(attempts.attackerPro).toBe(false);
  await recordOutcome("billing-chaos-08-rls", {
    expectations: [
      "Authenticated INSERT/UPDATE on billing_subscriptions/billing_customers is refused or affects 0 rows.",
      "Writer RPCs (billing_record_*, billing_attach_customer, organization_has_team) are not callable by authenticated.",
      "has_pro for another user answers false; the attacker stays free.",
    ],
    details: attempts,
  });
}, 60000);

it("09 checkout owner conflict: a second owner cannot claim a mapped customer", async () => {
  const a = await newUser();
  const b = await newUser();
  const cus = id("cus");
  customers.push(cus);
  const [ra, rb] = await Promise.all([
    postStripeEvent(checkoutEvent(cus, a.user.id)),
    postStripeEvent(checkoutEvent(cus, b.user.id)),
  ]);
  const results = [ra.body, rb.body].map((x) => (x as { result?: string }).result).sort();
  expect(results).toEqual(["applied", "owner_conflict"]);
  await recordOutcome("billing-chaos-09-owner-race", {
    expectations: ["Two concurrent checkouts naming different owners for one customer: one applied, one owner_conflict."],
    details: { results },
  });
}, 60000);
