// Unit tests for the SSH CA trust list (ssh-cert.ts) and writing it to a
// machine on every wake (machine-setup.ts), which is what lets the CA roll.
//
// Run with: `deno test supabase/functions/tests/ca_trust.test.ts`

import { assert, assertEquals, assertThrows } from "https://deno.land/std@0.224.0/assert/mod.ts";
import { caTrustLines, trustedCaKeysFromEnv } from "../_shared/remote/ssh-cert.ts";
import { installCaTrust, reensureServicesAfterWake } from "../_shared/remote/machine-setup.ts";
import { BOAT_CAPABILITIES } from "../_shared/remote/boat-adapter.ts";
import { base64Encode } from "../_shared/remote/ssh-wire.ts";
import { fakeProvider } from "./helpers/remote_fakes.ts";

const key = (fill: number) => new Uint8Array(32).fill(fill);
const b64 = (fill: number) => base64Encode(key(fill));

// The signing CA is key(1); `trusted` is the trust list secret, unset when undefined.
function useCa(trusted?: string) {
  Deno.env.set("REMOTE_SSH_CA_ED25519_SEED_BASE64", b64(9));
  Deno.env.set("REMOTE_SSH_CA_ED25519_PUBLIC_KEY_BASE64", b64(1));
  if (trusted === undefined) Deno.env.delete("REMOTE_SSH_CA_TRUSTED_PUBLIC_KEYS_BASE64");
  else Deno.env.set("REMOTE_SSH_CA_TRUSTED_PUBLIC_KEYS_BASE64", trusted);
}

Deno.test("with no trust list only the signing CA is trusted", () => {
  useCa();
  assertEquals(trustedCaKeysFromEnv(), [key(1)]);
});

Deno.test("the trust list adds the next CA during a roll", () => {
  useCa(`${b64(1)}, ${b64(2)}`);
  assertEquals(trustedCaKeysFromEnv(), [key(1), key(2)]);
});

Deno.test("a trust list without the signing CA is refused", () => {
  useCa(b64(2));
  assertThrows(() => trustedCaKeysFromEnv(), Error, "must include the signing CA");
});

Deno.test("a trust list entry that is not a 32-byte key is refused", () => {
  useCa(`${b64(1)},${base64Encode(new Uint8Array(31))}`);
  assertThrows(() => trustedCaKeysFromEnv(), Error, "32-byte");
});

Deno.test("each trusted CA becomes its own TrustedUserCAKeys line", () => {
  const lines = caTrustLines([key(1), key(2)]);
  assertEquals(lines.length, 2);
  assert(lines.every((l) => /^ssh-ed25519 \S+ treq-ssh-ca$/.test(l)), lines.join("\n"));
  assert(lines[0] !== lines[1]);
});

function recorder(serviceSupervisor: boolean) {
  const log: string[] = [];
  const provider = fakeProvider({
    kind: "boat",
    capabilities: () => ({ ...BOAT_CAPABILITIES, serviceSupervisor }),
    execOnMachine: (_id, cmd) => {
      log.push(`exec ${cmd.join(" ")}`);
      return Promise.resolve({ exitCode: 0, stdout: "", stderr: "" });
    },
    ensureService: (_id, name) => {
      log.push(`service ${name}`);
      return Promise.resolve();
    },
  });
  return { provider, log };
}

Deno.test("installCaTrust writes every trusted CA and returns their fingerprints", async () => {
  useCa(`${b64(1)},${b64(2)}`);
  const { provider, log } = recorder(false);
  const fingerprints = await installCaTrust(provider, "m1");
  assertEquals(fingerprints.length, 2);
  assert(fingerprints.every((f) => f.startsWith("SHA256:")), fingerprints.join());
  const [line1, line2] = caTrustLines([key(1), key(2)]);
  assert(log.length === 1 && log[0]?.includes(`${line1}\n${line2}`), log.join("\n"));
});

Deno.test("a wake rewrites the trusted CAs before sshd starts", async () => {
  useCa(`${b64(1)},${b64(2)}`);
  const { provider, log } = recorder(false);
  await reensureServicesAfterWake(provider, "m1", null);
  assertEquals(log.map((l) => l.split(" ")[0]), ["exec", "service"]);
  assert(log[0]?.includes("treq_ca.pub"), log[0]);
});

Deno.test("a wake rewrites the trusted CAs under a real service supervisor too", async () => {
  useCa();
  const { provider, log } = recorder(true);
  await reensureServicesAfterWake(provider, "m1", null);
  assertEquals(log.length, 1);
  assert(log[0]?.includes("treq_ca.pub"), log[0]);
});
