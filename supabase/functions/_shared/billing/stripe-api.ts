// A minimal Stripe REST client: form-encoded requests over fetch, so the
// Edge Functions need no Stripe SDK. Callers pass Stripe's bracketed keys
// directly, for example `line_items[0][price]`.

export type StripeParams = Record<string, string | number | boolean>;

export class StripeApiError extends Error {
  constructor(
    readonly status: number,
    message: string,
  ) {
    super(message);
    this.name = "StripeApiError";
  }
}

export interface StripeClient {
  request<T = Record<string, unknown>>(
    method: "GET" | "POST",
    path: string,
    params?: StripeParams,
    options?: { idempotencyKey?: string },
  ): Promise<T>;
}

const STRIPE_API_BASE = "https://api.stripe.com";

/**
 * Every request pins this API version instead of the account default, so the
 * request and response shapes cannot change under us when the account is
 * upgraded. It matches the release train of @stripe/stripe-js 8 on the web
 * dashboard, whose initEmbeddedCheckout expects `ui_mode=embedded`.
 */
export const STRIPE_API_VERSION = "2025-09-30.clover";

export function encodeStripeForm(params: StripeParams): string {
  const form = new URLSearchParams();
  for (const [key, value] of Object.entries(params)) {
    form.append(key, String(value));
  }
  return form.toString();
}

export function createStripeClient(
  secretKey: string,
  fetchImpl: typeof fetch = fetch,
): StripeClient {
  return {
    async request<T>(
      method: "GET" | "POST",
      path: string,
      params: StripeParams = {},
      options: { idempotencyKey?: string } = {},
    ): Promise<T> {
      const encoded = encodeStripeForm(params);
      const headers: Record<string, string> = {
        Authorization: `Bearer ${secretKey}`,
        "Stripe-Version": STRIPE_API_VERSION,
      };
      let url = `${STRIPE_API_BASE}${path}`;
      const init: RequestInit = { method, headers };
      if (method === "GET") {
        if (encoded) url += `?${encoded}`;
      } else {
        headers["Content-Type"] = "application/x-www-form-urlencoded";
        if (options.idempotencyKey) {
          headers["Idempotency-Key"] = options.idempotencyKey;
        }
        init.body = encoded;
      }

      const response = await fetchImpl(url, init);
      const body = (await response.json().catch(() => null)) as {
        error?: { message?: string };
      } | null;
      if (!response.ok) {
        throw new StripeApiError(
          response.status,
          body?.error?.message ?? `Stripe ${method} ${path} failed`,
        );
      }
      return body as T;
    },
  };
}
