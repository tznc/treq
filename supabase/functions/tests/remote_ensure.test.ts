// Unit tests for the ensure action's checks that run before any row or
// machine is created.
//
// Run with: `deno test supabase/functions/tests/remote_ensure.test.ts`

import { assertEquals } from "https://deno.land/std@0.224.0/assert/mod.ts";
import { handleEnsure, type RemoteInstanceDeps } from "../_shared/remote/instance-actions.ts";

// Any use of the database or a provider fails the test.
const untouched = <T extends object>(what: string): T =>
  new Proxy({} as T, {
    get() {
      throw new Error(`${what} was used`);
    },
  });

Deno.test("ensure is refused before anything is created when the SSH CA is not configured", async () => {
  Deno.env.delete("REMOTE_SSH_CA_ED25519_SEED_BASE64");
  Deno.env.delete("REMOTE_SSH_CA_ED25519_PUBLIC_KEY_BASE64");
  const deps: RemoteInstanceDeps = {
    supabase: untouched<RemoteInstanceDeps["supabase"]>("the database"),
    ownerUserId: "user-1",
    correlationId: "corr-1",
    provider: () => untouched("a provider"),
    stub: false,
    hasPro: () => Promise.resolve(true),
  };

  const reply = await handleEnsure({ idempotency_key: "k1" }, deps);

  assertEquals(reply, {
    status: 503,
    body: { error: "Treq Cloud is not set up to sign SSH certificates yet", code: "ssh_ca_not_configured" },
  });
});
