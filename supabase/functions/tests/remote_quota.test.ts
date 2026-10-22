// Local, non-network unit tests for the base resource quota (5 GB disk /
// 1 vCPU / 2 GB RAM) enforced by the Remote SSH control plane
// (prds/remote-development.md, "Instance lifecycle" > "Resource quotas").
//
// Unlike remote_e2e.test.ts, this suite never talks to a Supabase project
// or the Fly Sprites API - it exercises the pure catalog/adapter logic that
// decides what allocation is requested and validated, so it always runs
// (no TREQ_REMOTE_E2E gate, no network permissions needed).
//
// Run with: `deno test --allow-env supabase/functions/tests/remote_quota.test.ts`

import {
  assert,
  assertEquals,
  assertFalse,
} from "https://deno.land/std@0.224.0/assert/mod.ts";
import {
  BASE_ALLOCATION,
  BASE_DISK_QUOTA_BYTES,
  isBaseAllocationPreset,
  type SizePreset,
} from "../_shared/remote/catalog.ts";

Deno.test("base allocation matches the PRD fixed quota", () => {
  assertEquals(BASE_ALLOCATION.preset, "small");
  assertEquals(BASE_ALLOCATION.vcpu, 1);
  assertEquals(BASE_ALLOCATION.ramGb, 2);
  assertEquals(BASE_ALLOCATION.diskGb, 5);
  assertEquals(BASE_DISK_QUOTA_BYTES, 5 * 1024 * 1024 * 1024);
});

Deno.test("only the small preset is within the base allocation", () => {
  assert(isBaseAllocationPreset("small"));
  assertFalse(isBaseAllocationPreset("medium" as SizePreset));
  assertFalse(isBaseAllocationPreset("large" as SizePreset));
});

Deno.test("sprites adapter creates a deterministic sprite without Machines configuration", async () => {
  const { SpritesProvider } = await import(
    "../_shared/remote/sprites-adapter.ts"
  );
  const originalFetch = globalThis.fetch;
  const capturedBodies: unknown[] = [];
  globalThis.fetch = ((_input: RequestInfo | URL, init?: RequestInit) => {
    capturedBodies.push(JSON.parse(init?.body as string));
    return Promise.resolve(
      new Response(
        JSON.stringify({
          id: "sprite-id",
          name: "dev-treq-user-1",
          status: "warm",
        }),
        { status: 200 },
      ),
    );
  }) as typeof fetch;

  try {
    const provider = new SpritesProvider({
      baseUrl: "https://example.invalid",
      apiToken: "test-token",
    });
    const instance = await provider.createInstance({
      ownerUserId: "user-1",
      region: "us_east",
      sizePreset: "small",
      manifestVersion: 1,
      idempotencyKey: "idem-create",
    });
    assertEquals(capturedBodies, [{ name: "dev-treq-user-1" }]);
    assertEquals(instance.state, "ready");
  } finally {
    globalThis.fetch = originalFetch;
  }
});

Deno.test("sprites adapter executes argv through the documented HTTP endpoint", async () => {
  const { SpritesProvider } = await import(
    "../_shared/remote/sprites-adapter.ts"
  );
  const originalFetch = globalThis.fetch;
  let capturedUrl = "";
  let capturedMethod = "";
  globalThis.fetch = ((input: RequestInfo | URL, init?: RequestInit) => {
    capturedUrl = String(input);
    capturedMethod = init?.method ?? "GET";
    return Promise.resolve(new Response("installed\n", { status: 200 }));
  }) as typeof fetch;

  try {
    const provider = new SpritesProvider({
      baseUrl: "https://example.invalid",
      apiToken: "test-token",
    });
    const result = await provider.execOnMachine(
      "treq-user-1",
      ["bash", "-lc", "echo installed"],
      20,
    );
    assertEquals(result, {
      exitCode: 0,
      stdout: "installed\n",
      stderr: "",
    });
  } finally {
    globalThis.fetch = originalFetch;
  }

  assertEquals(capturedMethod, "POST");
  assertEquals(
    capturedUrl,
    "https://example.invalid/v1/sprites/treq-user-1/exec?cmd=bash&cmd=-lc&cmd=echo+installed&path=bash",
  );
});

Deno.test("sprites adapter wakes a paused sprite with a no-op exec, not a metadata GET", async () => {
  const { SpritesProvider } = await import(
    "../_shared/remote/sprites-adapter.ts"
  );
  const originalFetch = globalThis.fetch;
  const requests: { method: string; url: string }[] = [];
  globalThis.fetch = ((input: RequestInfo | URL, init?: RequestInit) => {
    requests.push({ method: init?.method ?? "GET", url: String(input) });
    return Promise.resolve(new Response("", { status: 200 }));
  }) as typeof fetch;

  try {
    const provider = new SpritesProvider({
      baseUrl: "https://example.invalid",
      apiToken: "test-token",
    });
    await provider.wakeInstance("treq-user-1");
  } finally {
    globalThis.fetch = originalFetch;
  }

  assertEquals(requests, [
    {
      method: "POST",
      url: "https://example.invalid/v1/sprites/treq-user-1/exec?cmd=true&path=true",
    },
  ]);
});
