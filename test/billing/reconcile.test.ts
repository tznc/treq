import { describe, expect, it, vi } from "vitest";
import { reconcileStaleSubscriptions } from "../../supabase/functions/_shared/billing/reconcile.ts";
import { StripeApiError } from "../../supabase/functions/_shared/billing/stripe-api.ts";

const NOW = 1_800_000_000;

function stripeSub(id: string, status: string, periodEnd: number) {
  return {
    id,
    customer: `cus_${id}`,
    status,
    cancel_at_period_end: false,
    cancel_at: null,
    trial_end: null,
    items: {
      data: [
        {
          current_period_end: periodEnd,
          price: { lookup_key: "treq_pro_monthly" },
        },
      ],
    },
  };
}

function deps(
  stale: string[],
  stripeById: Record<string, unknown>,
  writeResult = "applied",
) {
  const writes: Record<string, unknown>[] = [];
  return {
    writes,
    deps: {
      store: {
        staleSubscriptionIds: vi.fn(async () => stale),
        writeSnapshot: vi.fn(async (args: Record<string, unknown>) => {
          writes.push(args);
          return writeResult;
        }),
      },
      stripe: {
        request: vi.fn(async (_method: string, path: string) => {
          const id = decodeURIComponent(path.split("/").pop() ?? "");
          const value = stripeById[id];
          if (value instanceof Error) throw value;
          return value;
        }),
      } as never,
      nowSeconds: () => NOW,
    },
  };
}

describe("reconcileStaleSubscriptions", () => {
  it("writes Stripe's current copy of every stale subscription", async () => {
    const { deps: d, writes } = deps(["sub_a", "sub_b"], {
      sub_a: stripeSub("sub_a", "canceled", NOW - 86_400),
      sub_b: stripeSub("sub_b", "active", NOW + 30 * 86_400),
    });
    const result = await reconcileStaleSubscriptions(d);
    expect(result).toEqual({
      checked: 2,
      applied: 2,
      stale: 0,
      missing: [],
      failed: [],
    });
    expect(writes).toEqual([
      expect.objectContaining({
        p_subscription_id: "sub_a",
        p_customer_id: "cus_sub_a",
        p_plan: "pro",
        p_status: "canceled",
        p_fetched_at: new Date(NOW * 1000).toISOString(),
      }),
      expect.objectContaining({
        p_subscription_id: "sub_b",
        p_status: "active",
      }),
    ]);
  });

  it("reports a subscription Stripe no longer has, without writing", async () => {
    const { deps: d, writes } = deps(["sub_gone"], {
      sub_gone: new StripeApiError(404, "No such subscription"),
    });
    const result = await reconcileStaleSubscriptions(d);
    expect(result.failed).toEqual([]);
    expect(writes).toEqual([]);
    expect(d.store.writeSnapshot).not.toHaveBeenCalled();
    expect(result.missing).toEqual(["sub_gone"]);
  });

  it("keeps going when one Stripe read fails, and reports it", async () => {
    const { deps: d, writes } = deps(["sub_err", "sub_ok"], {
      sub_err: new Error("network"),
      sub_ok: stripeSub("sub_ok", "active", NOW + 86_400),
    });
    const result = await reconcileStaleSubscriptions(d);
    expect(result.failed).toEqual(["sub_err"]);
    expect(writes).toHaveLength(1);
  });
});
