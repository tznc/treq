// LocalProvider: the in-memory provider for multi-provider flows in local
// testing. No network.
//
// Run with: `deno test --allow-env supabase/functions/tests/local_provider.test.ts`

import { assertEquals, assertRejects } from "https://deno.land/std@0.224.0/assert/mod.ts";
import { LocalProvider, setLocalExecHandler } from "../_shared/remote/local-provider.ts";
import { ProviderError } from "../_shared/remote/provider.ts";

Deno.test("exec returns the scripted output from setLocalExecHandler", async () => {
  const provider = new LocalProvider();
  const { providerResourceId: id } = await provider.createInstance({
    ownerUserId: crypto.randomUUID(),
    region: "us_east",
    sizePreset: "small",
    manifestVersion: 1,
    idempotencyKey: "k",
  });
  const seen: string[][] = [];
  setLocalExecHandler((_id, command) => {
    seen.push(command);
    return { exitCode: 7, stdout: "preflight", stderr: "" };
  });
  try {
    assertEquals(await provider.execOnMachine(id, ["uname", "-m"]), {
      exitCode: 7,
      stdout: "preflight",
      stderr: "",
    });
    assertEquals(seen, [["uname", "-m"]]);
  } finally {
    setLocalExecHandler(null);
  }
  assertEquals((await provider.execOnMachine(id, ["true"])).exitCode, 0);
});

Deno.test("instances are shared across LocalProvider objects", async () => {
  const { providerResourceId: id } = await new LocalProvider().createInstance({
    ownerUserId: crypto.randomUUID(),
    region: "us_east",
    sizePreset: "small",
    manifestVersion: 1,
    idempotencyKey: "k",
    machineType: "default",
  });
  assertEquals((await new LocalProvider().getInstance(id)).providerResourceId, id);
});

Deno.test("gatewayEndpoint is unavailable", async () => {
  const err = await assertRejects(
    () => new LocalProvider().gatewayEndpoint("x"),
    ProviderError,
  );
  assertEquals(err.kind, "unavailable");
});
