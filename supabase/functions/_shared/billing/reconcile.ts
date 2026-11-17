// Re-reads subscriptions whose stored row still entitles although their
// period ended, and writes Stripe's current copy. A lost or permanently
// failed customer.subscription.* webhook otherwise leaves the row stale
// until the 30-day entitlement grace runs out. Run hourly by remote-admin's
// `reconcile_stale_subscriptions` action.

import { subscriptionSnapshotArgs } from "../../stripe-webhook/lib.ts";
import { StripeApiError, type StripeClient } from "./stripe-api.ts";

export interface ReconcileStore {
  staleSubscriptionIds(): Promise<string[]>;
  /** billing_record_subscription_snapshot: 'applied' or 'stale'. */
  writeSnapshot(args: Record<string, unknown>): Promise<string>;
}

export type ReconcileResult = {
  checked: number;
  applied: number;
  stale: number;
  /** Stripe answered 404: needs a person, nothing is written. */
  missing: string[];
  failed: string[];
};

export async function reconcileStaleSubscriptions(deps: {
  store: ReconcileStore;
  stripe: StripeClient;
  nowSeconds?: () => number;
}): Promise<ReconcileResult> {
  const result: ReconcileResult = {
    checked: 0,
    applied: 0,
    stale: 0,
    missing: [],
    failed: [],
  };
  for (const id of await deps.store.staleSubscriptionIds()) {
    result.checked++;
    try {
      const current = await deps.stripe.request<Record<string, unknown>>(
        "GET",
        `/v1/subscriptions/${encodeURIComponent(id)}`,
      );
      const nowSeconds = deps.nowSeconds?.() ?? Math.floor(Date.now() / 1000);
      const args = subscriptionSnapshotArgs(current, nowSeconds);
      if (!args) {
        result.failed.push(id);
        continue;
      }
      const written = await deps.store.writeSnapshot(args);
      if (written === "applied") result.applied++;
      else result.stale++;
    } catch (err) {
      if (err instanceof StripeApiError && err.status === 404) {
        result.missing.push(id);
      } else {
        result.failed.push(id);
      }
    }
  }
  return result;
}

type QueryResult = { data: unknown; error: { message: string } | null };

export function reconcileStoreFor(
  client: {
    rpc(fn: string, args: Record<string, unknown>): PromiseLike<QueryResult>;
  },
  limit = 200,
): ReconcileStore {
  return {
    async staleSubscriptionIds() {
      const { data, error } = await client.rpc("billing_stale_subscriptions", {
        p_limit: limit,
      });
      if (error) throw new Error(`stale subscriptions: ${error.message}`);
      return (data as string[] | null) ?? [];
    },
    async writeSnapshot(args) {
      const { data, error } = await client.rpc(
        "billing_record_subscription_snapshot",
        args,
      );
      if (error) throw new Error(`snapshot write: ${error.message}`);
      return String(data);
    },
  };
}
