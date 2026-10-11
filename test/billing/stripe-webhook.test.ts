import { describe, expect, it, vi } from "vitest";
import { signStripePayload } from "../../supabase/functions/_shared/billing/stripe-signature.ts";
import {
  actionForEvent,
  handleStripeWebhook,
  type WebhookDeps,
} from "../../supabase/functions/stripe-webhook/lib.ts";
import checkoutCompleted from "./fixtures/checkout.session.completed.json";
import subscriptionCreated from "./fixtures/customer.subscription.created.json";
import subscriptionDeleted from "./fixtures/customer.subscription.deleted.json";
import subscriptionUpdated from "./fixtures/customer.subscription.updated.json";

const OWNER_ID = "6f1c1f4e-6a8f-4a39-9d55-2b1f0b3f9a10";
const SECRET = "whsec_unit_test";

function iso(unixSeconds: number): string {
  return new Date(unixSeconds * 1000).toISOString();
}

// Fixtures are shared module objects, so every test edits a deep copy.
function copy<T>(value: T): T {
  return structuredClone(value);
}

describe("actionForEvent: subscriptions", () => {
  it("maps a trialing subscription with the period on its items", () => {
    expect(actionForEvent(subscriptionCreated)).toEqual({
      action: "rpc",
      fn: "billing_record_subscription_event",
      args: {
        p_event_id: "evt_1SubCreatedTrialing",
        p_event_type: "customer.subscription.created",
        p_event_created: iso(1790000000),
        p_subscription_id: "sub_1Trialing",
        p_customer_id: "cus_Treq123",
        p_plan: "pro",
        p_status: "trialing",
        p_current_period_end: iso(1791209600),
        p_cancel_at_period_end: false,
        p_trial_end: iso(1791209600),
      },
    });
  });

  it("maps a subscription set to cancel, with the period on the subscription", () => {
    expect(actionForEvent(subscriptionUpdated)).toMatchObject({
      action: "rpc",
      args: {
        p_event_type: "customer.subscription.updated",
        p_status: "active",
        p_current_period_end: iso(1793801600),
        p_cancel_at_period_end: true,
      },
    });
  });

  it("treats cancel_at on the period end as canceling", () => {
    const event = copy(subscriptionCreated);
    event.data.object.status = "active";
    event.data.object.cancel_at = 1791209600 as unknown as null;
    expect(actionForEvent(event)).toMatchObject({
      args: { p_cancel_at_period_end: true },
    });
  });

  it("does not treat a cancel_at after the period end as canceling", () => {
    const event = copy(subscriptionCreated);
    event.data.object.cancel_at = (1791209600 + 86_400 * 60) as unknown as null;
    expect(actionForEvent(event)).toMatchObject({
      args: { p_cancel_at_period_end: false },
    });
  });

  it("maps a deleted subscription as canceled", () => {
    expect(actionForEvent(subscriptionDeleted)).toMatchObject({
      action: "rpc",
      args: {
        p_event_type: "customer.subscription.deleted",
        p_status: "canceled",
        p_event_created: iso(1793801601),
      },
    });
  });

  it("forces canceled on deletion whatever status the object carries", () => {
    const event = copy(subscriptionDeleted);
    event.data.object.status = "incomplete_expired";
    expect(actionForEvent(event)).toMatchObject({
      args: { p_status: "canceled" },
    });
  });

  it("maps the Team lookup key to team", () => {
    const event = copy(subscriptionCreated);
    event.data.object.items.data[0].price.lookup_key = "treq_team_monthly";
    expect(actionForEvent(event)).toMatchObject({ args: { p_plan: "team" } });
  });

  it("reads an expanded customer object", () => {
    const event = copy(subscriptionCreated) as unknown as {
      data: { object: { customer: unknown } };
    };
    event.data.object.customer = { id: "cus_Expanded", object: "customer" };
    expect(actionForEvent(event)).toMatchObject({
      args: { p_customer_id: "cus_Expanded" },
    });
  });

  it("ignores a subscription with no Treq price", () => {
    const event = copy(subscriptionCreated);
    event.data.object.items.data[0].price.lookup_key = "someone_elses_price";
    expect(actionForEvent(event)).toEqual({
      action: "ignore",
      reason: "no Treq price on subscription",
    });
  });

  it("ignores a status Treq does not know", () => {
    const event = copy(subscriptionCreated);
    event.data.object.status = "on_vacation";
    expect(actionForEvent(event)).toMatchObject({ action: "ignore" });
  });

  it("ignores a subscription without a customer", () => {
    const event = copy(subscriptionCreated) as unknown as {
      data: { object: { customer: unknown } };
    };
    event.data.object.customer = null;
    expect(actionForEvent(event)).toMatchObject({ action: "ignore" });
  });
});

describe("actionForEvent: checkout.session.completed", () => {
  it("maps the customer to the owner in the session metadata", () => {
    expect(actionForEvent(checkoutCompleted)).toEqual({
      action: "rpc",
      fn: "billing_record_checkout_completed",
      args: {
        p_event_id: "evt_1CheckoutCompleted",
        p_customer_id: "cus_Treq123",
        p_owner_type: "user",
        p_owner_id: OWNER_ID,
      },
    });
  });

  it("ignores a session whose owner exists only in client_reference_id", () => {
    // A Payment Link accepts client_reference_id from its URL, so anyone can
    // set it. Only billing-checkout writes metadata.
    const event = copy(checkoutCompleted) as unknown as {
      data: { object: { metadata: unknown } };
    };
    event.data.object.metadata = {};
    expect(actionForEvent(event)).toMatchObject({ action: "ignore" });
  });

  it("ignores a session whose client_reference_id disagrees with its metadata", () => {
    const event = copy(checkoutCompleted);
    event.data.object.client_reference_id =
      "00000000-0000-0000-0000-000000000bad";
    expect(actionForEvent(event)).toMatchObject({ action: "ignore" });
  });

  it("ignores a session without client_reference_id", () => {
    const event = copy(checkoutCompleted) as unknown as {
      data: { object: { client_reference_id: unknown } };
    };
    event.data.object.client_reference_id = null;
    expect(actionForEvent(event)).toMatchObject({ action: "ignore" });
  });

  it("maps the customer to an organization owner for Team", () => {
    const event = copy(checkoutCompleted);
    event.data.object.metadata.owner_type = "organization";
    expect(actionForEvent(event)).toMatchObject({
      action: "rpc",
      fn: "billing_record_checkout_completed",
      args: { p_owner_type: "organization", p_owner_id: OWNER_ID },
    });
  });

  it("ignores an owner type it does not know", () => {
    const event = copy(checkoutCompleted);
    event.data.object.metadata.owner_type = "team";
    expect(actionForEvent(event)).toMatchObject({ action: "ignore" });
  });

  it("ignores an owner id that is not a UUID", () => {
    const event = copy(checkoutCompleted);
    event.data.object.metadata.owner_id = "not-a-uuid";
    event.data.object.client_reference_id = "not-a-uuid";
    expect(actionForEvent(event)).toMatchObject({ action: "ignore" });
  });

  it("ignores a one-off payment session", () => {
    const event = copy(checkoutCompleted);
    event.data.object.mode = "payment";
    expect(actionForEvent(event)).toMatchObject({ action: "ignore" });
  });
});

describe("actionForEvent: other events", () => {
  it("ignores event types the webhook does not handle", () => {
    const event = copy(subscriptionCreated);
    event.type = "invoice.paid";
    expect(actionForEvent(event)).toEqual({
      action: "ignore",
      reason: "unhandled event type invoice.paid",
    });
  });
});

describe("handleStripeWebhook", () => {
  const now = 1_790_000_100;

  function deps(overrides: Partial<WebhookDeps> = {}): WebhookDeps {
    return {
      secret: SECRET,
      rpc: vi.fn(async () => ({ data: "applied", error: null })),
      nowSeconds: () => now,
      log: () => {},
      ...overrides,
    };
  }

  async function signed(event: unknown, timestamp = now) {
    const body = JSON.stringify(event);
    return {
      method: "POST",
      body,
      signature: await signStripePayload(body, SECRET, timestamp),
    };
  }

  it("records a signed subscription event", async () => {
    const d = deps();
    const result = await handleStripeWebhook(
      await signed(subscriptionCreated),
      d,
    );
    expect(result.status).toBe(200);
    expect(JSON.parse(result.body)).toEqual({
      received: true,
      result: "applied",
    });
    expect(d.rpc).toHaveBeenCalledTimes(1);
    expect(d.rpc).toHaveBeenCalledWith(
      "billing_record_subscription_event",
      expect.objectContaining({
        p_event_id: "evt_1SubCreatedTrialing",
        p_plan: "pro",
      }),
    );
  });

  it("reports a replayed event as a duplicate", async () => {
    const d = deps({
      rpc: vi.fn(async () => ({ data: "duplicate", error: null })),
    });
    const result = await handleStripeWebhook(
      await signed(checkoutCompleted),
      d,
    );
    expect(result.status).toBe(200);
    expect(JSON.parse(result.body)).toEqual({
      received: true,
      result: "duplicate",
    });
  });

  it("rejects a bad signature without touching the database", async () => {
    const d = deps();
    const request = await signed(subscriptionCreated);
    const result = await handleStripeWebhook(
      {
        ...request,
        signature: request.signature.replace(
          /v1=[0-9a-f]+/,
          `v1=${"0".repeat(64)}`,
        ),
      },
      d,
    );
    expect(result.status).toBe(400);
    expect(d.rpc).not.toHaveBeenCalled();
  });

  it("rejects an event signed with another secret", async () => {
    const d = deps();
    const body = JSON.stringify(subscriptionCreated);
    const result = await handleStripeWebhook(
      {
        method: "POST",
        body,
        signature: await signStripePayload(body, "whsec_attacker", now),
      },
      d,
    );
    expect(result.status).toBe(400);
    expect(d.rpc).not.toHaveBeenCalled();
  });

  it("rejects a replay of a signed request after five minutes", async () => {
    const d = deps();
    const result = await handleStripeWebhook(
      await signed(subscriptionCreated, now - 301),
      d,
    );
    expect(result.status).toBe(400);
    expect(d.rpc).not.toHaveBeenCalled();
  });

  it("rejects an unsigned request", async () => {
    const d = deps();
    const result = await handleStripeWebhook(
      {
        method: "POST",
        body: JSON.stringify(subscriptionCreated),
        signature: null,
      },
      d,
    );
    expect(result.status).toBe(400);
    expect(d.rpc).not.toHaveBeenCalled();
  });

  it("fails closed when the secret is not configured", async () => {
    const d = deps({ secret: undefined });
    const result = await handleStripeWebhook(
      await signed(subscriptionCreated),
      d,
    );
    expect(result.status).toBe(500);
    expect(d.rpc).not.toHaveBeenCalled();
  });

  it("rejects methods other than POST", async () => {
    const result = await handleStripeWebhook(
      { method: "GET", body: "", signature: null },
      deps(),
    );
    expect(result.status).toBe(405);
  });

  it("rejects a signed body that is not JSON", async () => {
    const d = deps();
    const body = "not json";
    const result = await handleStripeWebhook(
      {
        method: "POST",
        body,
        signature: await signStripePayload(body, SECRET, now),
      },
      d,
    );
    expect(result.status).toBe(400);
    expect(d.rpc).not.toHaveBeenCalled();
  });

  it("rejects a signed body that is not a Stripe event", async () => {
    const d = deps();
    const result = await handleStripeWebhook(
      await signed({ id: 42, type: "customer.subscription.created" }),
      d,
    );
    expect(result.status).toBe(400);
    expect(d.rpc).not.toHaveBeenCalled();
  });

  it("acknowledges an ignored event without writing", async () => {
    const d = deps();
    const event = copy(subscriptionCreated);
    event.type = "invoice.paid";
    const result = await handleStripeWebhook(await signed(event), d);
    expect(result.status).toBe(200);
    expect(JSON.parse(result.body)).toMatchObject({
      received: true,
      ignored: "unhandled event type invoice.paid",
    });
    expect(d.rpc).not.toHaveBeenCalled();
  });

  it("returns 500 so Stripe retries when the write fails", async () => {
    const d = deps({
      rpc: vi.fn(async () => ({
        data: null,
        error: { message: "connection reset" },
      })),
    });
    const result = await handleStripeWebhook(
      await signed(subscriptionCreated),
      d,
    );
    expect(result.status).toBe(500);
  });

  it("logs an owner conflict loudly but acknowledges it", async () => {
    const log = vi.fn();
    const d = deps({
      rpc: vi.fn(async () => ({ data: "owner_conflict", error: null })),
      log,
    });
    const result = await handleStripeWebhook(
      await signed(checkoutCompleted),
      d,
    );
    expect(result.status).toBe(200);
    expect(log).toHaveBeenCalledWith(
      expect.objectContaining({
        level: "error",
        result: "owner_conflict",
        stripe_event_id: "evt_1CheckoutCompleted",
      }),
    );
  });
});
