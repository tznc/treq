import { supabase } from "./supabase";

// Edge Functions answer errors as `{ error: string }`. supabase-js wraps a
// non-2xx answer in an error whose `context` is the raw Response.
export async function functionErrorMessage(
  error: unknown,
  fallback: string,
): Promise<string> {
  const context = (error as { context?: unknown } | null)?.context;
  if (context instanceof Response) {
    const body = (await context
      .clone()
      .json()
      .catch(() => null)) as { error?: unknown } | null;
    if (typeof body?.error === "string") return body.error;
  }
  return fallback;
}

export type CheckoutRequest =
  | { plan: "pro" }
  | { plan: "team"; organization_id: string };

/**
 * Asks billing-checkout for a Stripe-hosted Checkout session URL: Pro for the
 * user, or Team for an organization they own.
 */
export async function fetchCheckoutUrl(
  request: CheckoutRequest = { plan: "pro" },
): Promise<string> {
  const { data, error } = await supabase.functions.invoke("billing-checkout", {
    body: request,
  });
  if (error) {
    throw new Error(await functionErrorMessage(error, "Could not start checkout"));
  }
  const url = (data as { url?: unknown } | null)?.url;
  if (typeof url !== "string") throw new Error("Could not start checkout");
  return url;
}

/**
 * Asks billing-portal for a Stripe customer portal URL: the user's own, or
 * with an organization id, that organization's.
 */
export async function fetchBillingPortalUrl(
  request: { organization_id?: string } = {},
): Promise<string> {
  const { data, error } = await supabase.functions.invoke("billing-portal", {
    body: request,
  });
  if (error) {
    throw new Error(
      await functionErrorMessage(error, "Could not open billing management"),
    );
  }
  const url = (data as { url?: unknown } | null)?.url;
  if (typeof url !== "string") throw new Error("Could not open billing management");
  return url;
}
