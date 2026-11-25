// Unit tests for the operator sweep that rewrites the trusted SSH CAs on
// running machines (ca-trust-refresh.ts). In-memory store and provider.
//
// Run with: `deno test supabase/functions/tests/ca_trust_refresh.test.ts`

import { assert, assertEquals } from "https://deno.land/std@0.224.0/assert/mod.ts";
import { type CaTrustRefreshStore, refreshCaTrust, type RunningMachine } from "../_shared/remote/ca-trust-refresh.ts";
import { fakeProvider } from "./helpers/remote_fakes.ts";

function useCa() {
  Deno.env.set("REMOTE_SSH_CA_ED25519_SEED_BASE64", btoa("s".repeat(32)));
  Deno.env.set("REMOTE_SSH_CA_ED25519_PUBLIC_KEY_BASE64", btoa("p".repeat(32)));
  Deno.env.delete("REMOTE_SSH_CA_TRUSTED_PUBLIC_KEYS_BASE64");
}

const machine = (n: number): RunningMachine => ({
  instanceId: `i${n}`,
  ownerUserId: `u${n}`,
  endpointId: `e${n}`,
  placementId: `pl${n}`,
  providerKind: "boat",
  providerResourceId: `m${n}`,
});

function memoryStore(machines: RunningMachine[]) {
  const installed: Array<[string, string[]]> = [];
  const failed: Array<[string, string]> = [];
  const store: CaTrustRefreshStore = {
    runningMachines: (limit, after) =>
      Promise.resolve(machines.filter((m) => after === null || m.instanceId > after).slice(0, limit)),
    recordInstalled: (m, fingerprints) => {
      installed.push([m.instanceId, fingerprints]);
      return Promise.resolve();
    },
    recordFailed: (m, error) => {
      failed.push([m.instanceId, error]);
      return Promise.resolve();
    },
  };
  return { store, installed, failed };
}

// Machine `broken` refuses every command.
function providers(broken?: string) {
  const execs: string[] = [];
  const provider = fakeProvider({
    execOnMachine: (id) => {
      execs.push(id);
      const exitCode = id === broken ? 1 : 0;
      return Promise.resolve({ exitCode, stdout: "", stderr: exitCode ? "sudo: a password is required" : "" });
    },
  });
  return { providers: () => provider, execs };
}

Deno.test("each running machine gets the trusted CAs and its fingerprints are recorded", async () => {
  useCa();
  const { store, installed, failed } = memoryStore([machine(1), machine(2)]);
  const { providers: p, execs } = providers();
  const result = await refreshCaTrust(store, p, 50);
  assertEquals(execs, ["m1", "m2"]);
  assertEquals(installed.map(([id]) => id), ["i1", "i2"]);
  assert(installed.every(([, fps]) => fps.length === 1 && fps[0]?.startsWith("SHA256:")));
  assertEquals(failed, []);
  assertEquals(result, { refreshed: 2, failed: [], next_after: null });
});

Deno.test("a machine that fails is recorded and reported, and the rest still refresh", async () => {
  useCa();
  const { store, installed, failed } = memoryStore([machine(1), machine(2), machine(3)]);
  const { providers: p } = providers("m2");
  const result = await refreshCaTrust(store, p, 50);
  assertEquals(installed.map(([id]) => id), ["i1", "i3"]);
  assertEquals(failed.map(([id]) => id), ["i2"]);
  assertEquals(result.refreshed, 2);
  assertEquals(result.failed.map((f) => f.instance_id), ["i2"]);
  assert(result.failed[0]?.error.includes("password is required"), result.failed[0]?.error);
});

Deno.test("the sweep pages through machines with next_after", async () => {
  useCa();
  const { store, installed } = memoryStore([machine(1), machine(2), machine(3)]);
  const { providers: p } = providers();
  const first = await refreshCaTrust(store, p, 2);
  assertEquals(first.next_after, "i2");
  const second = await refreshCaTrust(store, p, 2, first.next_after);
  assertEquals(second.next_after, null);
  assertEquals(installed.map(([id]) => id), ["i1", "i2", "i3"]);
});
