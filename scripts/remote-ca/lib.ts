// Shared by the SSH CA operator scripts (generate.ts, verify.ts, roll.ts).
// A CA file holds the two secrets as NAME=value lines, the same format
// `supabase secrets set --env-file` reads. The seed never leaves the file.

export const SEED_SECRET = "REMOTE_SSH_CA_ED25519_SEED_BASE64";
export const PUBLIC_SECRET = "REMOTE_SSH_CA_ED25519_PUBLIC_KEY_BASE64";
export const TRUSTED_SECRET = "REMOTE_SSH_CA_TRUSTED_PUBLIC_KEYS_BASE64";

/** Base64 raw 32-byte values; `seed` is absent in a public-only file. */
export interface CaFile {
  seed?: string;
  publicKey: string;
}

const b64 = (bytes: Uint8Array) => btoa(String.fromCharCode(...bytes));

export async function generateCa(): Promise<CaFile> {
  const pair = await crypto.subtle.generateKey("Ed25519", true, ["sign", "verify"]) as CryptoKeyPair;
  // An ed25519 PKCS8 key ends with the 32-byte seed.
  const pkcs8 = new Uint8Array(await crypto.subtle.exportKey("pkcs8", pair.privateKey));
  const publicKey = new Uint8Array(await crypto.subtle.exportKey("raw", pair.publicKey));
  return { seed: b64(pkcs8.slice(-32)), publicKey: b64(publicKey) };
}

export function formatCaFile(ca: CaFile): string {
  return (ca.seed ? `${SEED_SECRET}=${ca.seed}\n` : "") + `${PUBLIC_SECRET}=${ca.publicKey}\n`;
}

export function parseCaFile(text: string): CaFile {
  const values = new Map(
    text.split("\n").filter((l) => l.includes("=")).map((l) => {
      const i = l.indexOf("=");
      return [l.slice(0, i).trim(), l.slice(i + 1).trim()] as const;
    }),
  );
  const publicKey = values.get(PUBLIC_SECRET);
  if (!publicKey) throw new Error(`the CA file has no ${PUBLIC_SECRET}`);
  const seed = values.get(SEED_SECRET);
  return seed ? { seed, publicKey } : { publicKey };
}

/** The remote-admin `ca_status` answer (supabase/functions/_shared/remote/ca-status.ts). */
export type CaStatus = { ca_error: string } | {
  signing_fingerprint: string;
  trusted_fingerprints: string[];
  seed_matches_public: boolean;
  running_machines: number;
  stale_machines: Array<{ instance_id: string; fingerprints: string[] | null }>;
};

/** What is wrong with `status` against the expected fingerprints; empty when nothing is. */
export function checkCaStatus(status: CaStatus, expected: { signing: string; trusted: string[] }): string[] {
  if ("ca_error" in status) return [`the functions cannot load the CA: ${status.ca_error}`];
  const problems: string[] = [];
  if (status.signing_fingerprint !== expected.signing) {
    problems.push(`signing CA is ${status.signing_fingerprint}, expected ${expected.signing}`);
  }
  if (!status.seed_matches_public) problems.push("the seed does not match the signing public key");
  const sorted = (fps: string[]) => [...fps].sort().join(", ");
  if (sorted(status.trusted_fingerprints) !== sorted(expected.trusted)) {
    problems.push(`trusted CAs are [${sorted(status.trusted_fingerprints)}], expected [${sorted(expected.trusted)}]`);
  }
  if (status.stale_machines.length > 0) {
    const ids = status.stale_machines.map((m) => m.instance_id).join(", ");
    problems.push(`running machines still on another trust set (run roll.ts sweep): ${ids}`);
  }
  return problems;
}

/** Calls a remote-admin action with the operator key, as pg_cron does. */
export async function adminCall(action: string, body: Record<string, unknown> = {}): Promise<unknown> {
  const url = Deno.env.get("TREQ_SUPABASE_URL");
  const key = Deno.env.get("REMOTE_ADMIN_API_KEY");
  if (!url || !key) throw new Error("set TREQ_SUPABASE_URL and REMOTE_ADMIN_API_KEY");
  const anon = Deno.env.get("SUPABASE_ANON_KEY");
  const response = await fetch(`${url.replace(/\/$/, "")}/functions/v1/remote-admin`, {
    method: "POST",
    headers: {
      "content-type": "application/json",
      "x-admin-api-key": key,
      ...(anon ? { authorization: `Bearer ${anon}` } : {}),
    },
    body: JSON.stringify({ action, operator_id: "remote-ca-script", ...body }),
  });
  const text = await response.text();
  if (!response.ok) throw new Error(`remote-admin ${action} answered ${response.status}: ${text}`);
  return JSON.parse(text);
}
