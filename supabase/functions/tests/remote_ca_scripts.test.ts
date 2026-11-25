// Unit tests for the SSH CA operator scripts' shared logic
// (scripts/remote-ca/lib.ts): CA files and the verify checks.
//
// Run with: `deno test supabase/functions/tests/remote_ca_scripts.test.ts`

import { assert, assertEquals, assertMatch, assertThrows } from "https://deno.land/std@0.224.0/assert/mod.ts";
import { checkCaStatus, formatCaFile, generateCa, parseCaFile, planRoll } from "../../../scripts/remote-ca/lib.ts";
import { caKeyPairMatches } from "../_shared/remote/ssh-cert.ts";
import { base64Decode } from "../_shared/remote/ssh-wire.ts";

Deno.test("a generated CA is a matching ed25519 pair that survives its file format", async () => {
  const ca = await generateCa();
  const file = formatCaFile(ca);
  assertMatch(file, /^REMOTE_SSH_CA_ED25519_SEED_BASE64=\S+\nREMOTE_SSH_CA_ED25519_PUBLIC_KEY_BASE64=\S+\n$/);
  assertEquals(parseCaFile(file), ca);
  const material = { privateSeed: base64Decode(ca.seed ?? ""), publicKey: base64Decode(ca.publicKey) };
  assert(await caKeyPairMatches(material));
});

Deno.test("a CA file without a public key is refused", () => {
  assertThrows(() => parseCaFile("REMOTE_SSH_CA_ED25519_SEED_BASE64=abc\n"), Error, "PUBLIC_KEY");
});

const healthy = {
  signing_fingerprint: "SHA256:new",
  trusted_fingerprints: ["SHA256:new"],
  seed_matches_public: true,
  running_machines: 2,
  stale_machines: [],
};

Deno.test("verify passes when the functions see the expected CA and every running machine trusts it", () => {
  assertEquals(checkCaStatus(healthy, { signing: "SHA256:new", trusted: ["SHA256:new"] }), []);
});

Deno.test("verify reports each way a roll can be wrong", () => {
  const problems = checkCaStatus(
    {
      ...healthy,
      signing_fingerprint: "SHA256:old",
      seed_matches_public: false,
      trusted_fingerprints: ["SHA256:old", "SHA256:new"],
      stale_machines: [{ instance_id: "i1", fingerprints: ["SHA256:old"] }],
    },
    { signing: "SHA256:new", trusted: ["SHA256:new"] },
  );
  assertEquals(problems.length, 4, problems.join("\n"));
  assert(problems.some((p) => p.includes("signing CA is SHA256:old")));
  assert(problems.some((p) => p.includes("seed does not match")));
  assert(problems.some((p) => p.includes("trusted CAs are")));
  assert(problems.some((p) => p.includes("i1") && p.includes("sweep")));
});

Deno.test("verify ignores the order of the trusted CAs", () => {
  const status = { ...healthy, trusted_fingerprints: ["SHA256:b", "SHA256:new"] };
  assertEquals(checkCaStatus(status, { signing: "SHA256:new", trusted: ["SHA256:new", "SHA256:b"] }), []);
});

Deno.test("verify reports a CA the functions cannot load", () => {
  assertEquals(checkCaStatus({ ca_error: "secrets must be set" }, { signing: "x", trusted: ["x"] }), [
    "the functions cannot load the CA: secrets must be set",
  ]);
});

const current = { seed: "c-seed", publicKey: "c-pub" };
const next = { seed: "n-seed", publicKey: "n-pub" };

Deno.test("stage trusts both CAs and sweeps, keeping the current signer", () => {
  assertEquals(planRoll({ step: "stage", current, next }), {
    set: { REMOTE_SSH_CA_TRUSTED_PUBLIC_KEYS_BASE64: "c-pub,n-pub" },
    unset: [],
    sweep: true,
    expect: { signing: "c-pub", trusted: ["c-pub", "n-pub"] },
  });
});

Deno.test("promote signs with the next CA while both stay trusted", () => {
  assertEquals(planRoll({ step: "promote", current, next }), {
    set: { REMOTE_SSH_CA_ED25519_SEED_BASE64: "n-seed", REMOTE_SSH_CA_ED25519_PUBLIC_KEY_BASE64: "n-pub" },
    unset: [],
    sweep: false,
    expect: { signing: "n-pub", trusted: ["c-pub", "n-pub"] },
  });
});

Deno.test("retire drops the trust list so only the signer is trusted, and sweeps", () => {
  assertEquals(planRoll({ step: "retire", next }), {
    set: {},
    unset: ["REMOTE_SSH_CA_TRUSTED_PUBLIC_KEYS_BASE64"],
    sweep: true,
    expect: { signing: "n-pub", trusted: ["n-pub"] },
  });
});

Deno.test("emergency signs with and trusts only the next CA at once, and sweeps", () => {
  assertEquals(planRoll({ step: "emergency", next }), {
    set: { REMOTE_SSH_CA_ED25519_SEED_BASE64: "n-seed", REMOTE_SSH_CA_ED25519_PUBLIC_KEY_BASE64: "n-pub" },
    unset: ["REMOTE_SSH_CA_TRUSTED_PUBLIC_KEYS_BASE64"],
    sweep: true,
    expect: { signing: "n-pub", trusted: ["n-pub"] },
  });
});

Deno.test("a step that signs with a CA needs its seed", () => {
  assertThrows(() => planRoll({ step: "promote", current, next: { publicKey: "n-pub" } }), Error, "seed");
  assertThrows(() => planRoll({ step: "stage", next }), Error, "current");
});
