// Unit tests for what remote-admin `ca_status` reports about the SSH CA
// (ca-status.ts), which the verify script checks after a CA roll.
//
// Run with: `deno test supabase/functions/tests/ca_status.test.ts`

import { assert, assertEquals } from "https://deno.land/std@0.224.0/assert/mod.ts";
import { caStatus, type MachineTrust } from "../_shared/remote/ca-status.ts";
import { caFingerprint } from "../_shared/remote/ssh-cert.ts";
import { base64Encode } from "../_shared/remote/ssh-wire.ts";

// A real ed25519 pair, so the seed can be checked against its public key.
async function keyPair(): Promise<{ seed: Uint8Array; publicKey: Uint8Array }> {
  const pair = await crypto.subtle.generateKey("Ed25519", true, ["sign", "verify"]) as CryptoKeyPair;
  const pkcs8 = new Uint8Array(await crypto.subtle.exportKey("pkcs8", pair.privateKey));
  const publicKey = new Uint8Array(await crypto.subtle.exportKey("raw", pair.publicKey));
  return { seed: pkcs8.slice(-32), publicKey };
}

function useCa(seed: Uint8Array, publicKey: Uint8Array, trusted?: Uint8Array[]) {
  Deno.env.set("REMOTE_SSH_CA_ED25519_SEED_BASE64", base64Encode(seed));
  Deno.env.set("REMOTE_SSH_CA_ED25519_PUBLIC_KEY_BASE64", base64Encode(publicKey));
  if (trusted) Deno.env.set("REMOTE_SSH_CA_TRUSTED_PUBLIC_KEYS_BASE64", trusted.map(base64Encode).join(","));
  else Deno.env.delete("REMOTE_SSH_CA_TRUSTED_PUBLIC_KEYS_BASE64");
}

const store = (machines: MachineTrust[]) => ({ runningMachineTrust: () => Promise.resolve(machines) });

Deno.test("ca_status reports the signing and trusted fingerprints and a matching seed", async () => {
  const ca = await keyPair();
  const next = await keyPair();
  useCa(ca.seed, ca.publicKey, [ca.publicKey, next.publicKey]);
  const status = await caStatus(store([]));
  assertEquals(status, {
    signing_fingerprint: await caFingerprint(ca.publicKey),
    trusted_fingerprints: [await caFingerprint(ca.publicKey), await caFingerprint(next.publicKey)],
    seed_matches_public: true,
    running_machines: 0,
    stale_machines: [],
  });
});

Deno.test("ca_status catches a seed that does not match its public key", async () => {
  const ca = await keyPair();
  const other = await keyPair();
  useCa(other.seed, ca.publicKey);
  const status = await caStatus(store([]));
  assert("seed_matches_public" in status && status.seed_matches_public === false);
});

Deno.test("ca_status lists running machines whose last recorded trust differs", async () => {
  const ca = await keyPair();
  const old = await keyPair();
  useCa(ca.seed, ca.publicKey);
  const current = await caFingerprint(ca.publicKey);
  const stale = await caFingerprint(old.publicKey);
  const status = await caStatus(store([
    { instanceId: "i1", fingerprints: [current] },
    { instanceId: "i2", fingerprints: [stale, current] },
    { instanceId: "i3", fingerprints: null },
  ]));
  assert("stale_machines" in status);
  assertEquals(status.running_machines, 3);
  assertEquals(status.stale_machines, [
    { instance_id: "i2", fingerprints: [stale, current] },
    { instance_id: "i3", fingerprints: null },
  ]);
});

Deno.test("ca_status reports a missing CA instead of failing", async () => {
  Deno.env.delete("REMOTE_SSH_CA_ED25519_SEED_BASE64");
  Deno.env.delete("REMOTE_SSH_CA_ED25519_PUBLIC_KEY_BASE64");
  const status = await caStatus(store([]));
  assert("ca_error" in status && status.ca_error.includes("must be set"), JSON.stringify(status));
});
