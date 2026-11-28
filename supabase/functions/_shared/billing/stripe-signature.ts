// Stripe webhook signature verification with WebCrypto, so the webhook needs
// no Stripe SDK. Stripe signs `${t}.${raw body}` with HMAC-SHA256 under the
// endpoint secret and sends `Stripe-Signature: t=<unix>,v1=<hex>[,v1=<hex>]`.
// While a secret is being rolled, Stripe sends one v1 per active secret, so
// any matching v1 is enough. v0 is a legacy scheme and never counts.
// https://docs.stripe.com/webhooks#verify-manually

export const SIGNATURE_TOLERANCE_SECONDS = 300;

export type SignatureFailure =
  | "missing_header"
  | "malformed_header"
  | "no_matching_signature"
  | "timestamp_out_of_tolerance";

export type SignatureCheck =
  | { ok: true; timestamp: number }
  | { ok: false; reason: SignatureFailure };

type ParsedHeader = { timestamp: number | null; v1: string[] };

function parseSignatureHeader(header: string): ParsedHeader {
  let timestamp: number | null = null;
  const v1: string[] = [];
  for (const part of header.split(",")) {
    const eq = part.indexOf("=");
    if (eq < 0) continue;
    const key = part.slice(0, eq).trim();
    const value = part.slice(eq + 1).trim();
    if (key === "t" && timestamp === null && /^\d+$/.test(value)) {
      timestamp = Number(value);
    } else if (key === "v1" && value.length > 0) {
      v1.push(value);
    }
  }
  return { timestamp, v1 };
}

async function hmacSha256Hex(secret: string, message: string): Promise<string> {
  const encoder = new TextEncoder();
  const key = await crypto.subtle.importKey(
    "raw",
    encoder.encode(secret),
    { name: "HMAC", hash: "SHA-256" },
    false,
    ["sign"],
  );
  const mac = await crypto.subtle.sign("HMAC", key, encoder.encode(message));
  return Array.from(new Uint8Array(mac))
    .map((b) => b.toString(16).padStart(2, "0"))
    .join("");
}

// Compares every character, so the time taken does not reveal how many
// leading characters of a forged signature were right. The expected value is
// always 64 hex characters, so a length mismatch leaks nothing secret.
function timingSafeEqual(a: string, b: string): boolean {
  if (a.length !== b.length) return false;
  let diff = 0;
  for (let i = 0; i < a.length; i++) {
    diff |= a.charCodeAt(i) ^ b.charCodeAt(i);
  }
  return diff === 0;
}

/**
 * Checks a `Stripe-Signature` header against the raw request body. Pass the
 * body exactly as received: re-serialized JSON does not verify.
 */
export async function verifyStripeSignature(
  payload: string,
  header: string | null | undefined,
  secret: string,
  options: { nowSeconds?: number | undefined; toleranceSeconds?: number } = {},
): Promise<SignatureCheck> {
  if (!secret) {
    throw new Error("Stripe webhook secret is required");
  }
  if (!header) return { ok: false, reason: "missing_header" };

  const { timestamp, v1 } = parseSignatureHeader(header);
  if (timestamp === null || v1.length === 0) {
    return { ok: false, reason: "malformed_header" };
  }

  const expected = await hmacSha256Hex(secret, `${timestamp}.${payload}`);
  let matched = false;
  for (const candidate of v1) {
    // No early exit: every candidate is compared.
    matched = timingSafeEqual(expected, candidate) || matched;
  }
  if (!matched) return { ok: false, reason: "no_matching_signature" };

  // The timestamp is inside the signed message, so it is trustworthy here.
  // The window bounds how long a captured request can be replayed. Event ids
  // in billing_events stop replays inside the window.
  const now = options.nowSeconds ?? Math.floor(Date.now() / 1000);
  const tolerance = options.toleranceSeconds ?? SIGNATURE_TOLERANCE_SECONDS;
  if (Math.abs(now - timestamp) > tolerance) {
    return { ok: false, reason: "timestamp_out_of_tolerance" };
  }

  return { ok: true, timestamp };
}

/** Builds the header Stripe would send. Used by tests and service-qa. */
export async function signStripePayload(
  payload: string,
  secret: string,
  timestamp: number,
): Promise<string> {
  return `t=${timestamp},v1=${await hmacSha256Hex(secret, `${timestamp}.${payload}`)}`;
}
