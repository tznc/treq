// Request logic for billing-portal, free of Deno and Supabase imports so it
// runs under the unit tests and service-qa. index.ts verifies the user's JWT
// and supplies the stores and Stripe client.
//
// Without an organization id the portal opens for the user's own customer.
// With one, it opens for the organization's customer, and only for an
// owner.

import {
  type BillingOwnerType,
  dashboardBillingUrl,
  UUID_PATTERN,
} from "../_shared/billing/checkout.ts";
import type {
  BillingStore,
  OrganizationBilling,
} from "../_shared/billing/store.ts";
import {
  StripeApiError,
  type StripeClient,
} from "../_shared/billing/stripe-api.ts";

export type PortalDeps = {
  store: Pick<BillingStore, "getCustomer">;
  organizations?: OrganizationBilling;
  /** `organization_id` from the request body, if any. */
  organizationId?: unknown;
  stripe: StripeClient;
  webUrl: string;
};

type PortalResult = { status: number; body: Record<string, unknown> };

async function portalOwner(
  deps: PortalDeps,
): Promise<
  | {
      ok: true;
      type: BillingOwnerType;
      store: Pick<BillingStore, "getCustomer">;
    }
  | { ok: false; result: PortalResult }
> {
  if (deps.organizationId === undefined || deps.organizationId === null) {
    return { ok: true, type: "user", store: deps.store };
  }
  const organizationId = deps.organizationId;
  if (
    typeof organizationId !== "string" ||
    !UUID_PATTERN.test(organizationId)
  ) {
    return {
      ok: false,
      result: {
        status: 400,
        body: { error: "organization_id must be a UUID" },
      },
    };
  }
  if (!deps.organizations)
    throw new Error("organization billing is not configured");
  if (!(await deps.organizations.isOwner(organizationId))) {
    return {
      ok: false,
      result: {
        status: 403,
        body: {
          error: "Only an owner of this organization can manage its billing.",
        },
      },
    };
  }
  return {
    ok: true,
    type: "organization",
    store: deps.organizations.store(organizationId),
  };
}

export async function createPortal(deps: PortalDeps): Promise<PortalResult> {
  const owner = await portalOwner(deps);
  if (!owner.ok) return owner.result;

  const customer = await owner.store.getCustomer();
  if (!customer) {
    return {
      status: 404,
      body: {
        error:
          owner.type === "user"
            ? "No billing account for this user"
            : "No billing account for this organization",
      },
    };
  }

  try {
    const session = await deps.stripe.request<{ url: string }>(
      "POST",
      "/v1/billing_portal/sessions",
      {
        customer: customer.stripe_customer_id,
        return_url: dashboardBillingUrl(deps.webUrl, owner.type),
      },
    );
    return { status: 200, body: { url: session.url } };
  } catch (err) {
    if (err instanceof StripeApiError) {
      console.error(`[billing-portal] Stripe ${err.status}: ${err.message}`);
      return { status: 502, body: { error: "Stripe request failed" } };
    }
    throw err;
  }
}
