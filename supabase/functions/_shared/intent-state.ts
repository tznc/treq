// Single-use intent states for the GitHub App install and Linear OAuth
// flows. The opaque state goes to the browser; only its SHA-256 hash is
// stored, so a leaked table row cannot be replayed.

export const INTENT_TTL_MINUTES = 15;

/** 256 bits of entropy, hex encoded. */
export function newIntentState(): string {
  return Array.from(crypto.getRandomValues(new Uint8Array(32)))
    .map((b) => b.toString(16).padStart(2, "0"))
    .join("");
}

export async function sha256Hex(input: string): Promise<string> {
  const digest = await crypto.subtle.digest(
    "SHA-256",
    new TextEncoder().encode(input),
  );
  return Array.from(new Uint8Array(digest))
    .map((b) => b.toString(16).padStart(2, "0"))
    .join("");
}

export function intentExpiry(now: number): string {
  return new Date(now + INTENT_TTL_MINUTES * 60 * 1000).toISOString();
}

export type IntentRow = {
  user_id: string;
  state_hash: string;
  expires_at: string;
};

export type HandlerResult = {
  status: number;
  body: Record<string, unknown>;
};
