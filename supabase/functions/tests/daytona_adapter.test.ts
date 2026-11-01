// Daytona adapter request shapes, checked against an in-memory fake of
// Daytona's REST and toolbox APIs (daytona.io/docs/openapi.json,
// daytona.io/docs/toolbox-openapi.json). No network.
//
// Run with: `deno test --allow-env supabase/functions/tests/daytona_adapter.test.ts`

import { defined } from "./remote_fakes.ts";
import {
  assert,
  assertEquals,
  assertRejects,
} from "https://deno.land/std@0.224.0/assert/mod.ts";
import {
  DAYTONA_CAPABILITIES,
  DaytonaProvider,
  daytonaConfigFromEnv,
  normalizeDaytonaState,
} from "../_shared/remote/daytona-adapter.ts";
import { type CreateInstanceParams, ProviderError } from "../_shared/remote/provider.ts";
import { FakeDaytona } from "./fake_daytona.ts";

const CONFIG = { baseUrl: "https://daytona.example.invalid/api/", apiKey: "dtn_test" };

function setup() {
  const fake = new FakeDaytona();
  const provider = new DaytonaProvider(CONFIG, { fetch: fake.fetch });
  return { fake, provider };
}

const PARAMS: CreateInstanceParams = {
  ownerUserId: "Owner_1",
  region: "us_east",
  sizePreset: "large",
  manifestVersion: 3,
  idempotencyKey: "idem-1",
  machineType: "m",
};

Deno.test("daytonaConfigFromEnv needs the key and defaults the URL", () => {
  const err = (() => {
    try {
      daytonaConfigFromEnv(() => undefined);
    } catch (e) {
      return e;
    }
  })();
  assert(err instanceof ProviderError);
  assertEquals(err.kind, "invalid_request");
  assertEquals(daytonaConfigFromEnv((k: string) => ({ TREQ_DAYTONA_API_KEY: "k" } as Record<string, string>)[k]), {
    baseUrl: "https://app.daytona.io/api",
    apiKey: "k",
    autoStopMinutes: 60,
  });
  assertEquals(
    daytonaConfigFromEnv((k: string) => ({ TREQ_DAYTONA_API_KEY: "k", TREQ_DAYTONA_API_URL: "https://u" } as Record<string, string>)[k]),
    { baseUrl: "https://u", apiKey: "k", autoStopMinutes: 60 },
  );
  assertEquals(
    daytonaConfigFromEnv((k: string) => ({ TREQ_DAYTONA_API_KEY: "k", TREQ_PROVIDER_AUTOSTOP_MIN: "120" } as Record<string, string>)[k])
      .autoStopMinutes,
    120,
  );
});

Deno.test("capabilities: per-second billing, stop keeps disk", () => {
  assertEquals(DAYTONA_CAPABILITIES.billingGranularitySec, 1);
  assert(DAYTONA_CAPABILITIES.stopKeepsDisk);
  assert(DAYTONA_CAPABILITIES.execApi);
  assertEquals(DAYTONA_CAPABILITIES.serviceSupervisor, false);
});

Deno.test("states normalize to the provider-neutral set", () => {
  assertEquals(normalizeDaytonaState("creating"), "provisioning");
  assertEquals(normalizeDaytonaState("started"), "ready");
  assertEquals(normalizeDaytonaState("starting"), "waking");
  assertEquals(normalizeDaytonaState("stopped"), "suspended");
  assertEquals(normalizeDaytonaState("archived"), "suspended");
  assertEquals(normalizeDaytonaState("error"), "failed");
  assertEquals(normalizeDaytonaState("build_failed"), "failed");
  assertEquals(normalizeDaytonaState("destroying"), "deleting");
  assertEquals(normalizeDaytonaState("destroyed"), "deleted");
  assertEquals(normalizeDaytonaState("something_new"), "degraded");
});

Deno.test("create looks up by labels, then POSTs the tier's resources with the auto-stop backstop", async () => {
  const { fake, provider } = setup();
  const instance = await provider.createInstance(PARAMS);
  const [lookup, create] = fake.requests;
  assertEquals(lookup.method, "GET");
  assertEquals(lookup.path, "/api/sandbox");
  assertEquals(JSON.parse(defined(lookup.search.get("labels"))), { treq_owner: "owner_1", treq_placement: "idem-1" });
  assertEquals(create.method, "POST");
  assertEquals(create.path, "/api/sandbox");
  assertEquals(create.headers.get("Authorization"), "Bearer dtn_test");
  assertEquals(create.body, {
    cpu: 4,
    memory: 8,
    disk: 40,
    target: "us",
    autoStopInterval: 60,
    autoArchiveInterval: 0,
    autoDeleteInterval: -1,
    labels: { treq_owner: "owner_1", treq_placement: "idem-1", treq_manifest: "3" },
  });
  assertEquals(instance.providerResourceId, "dt_1");
  assertEquals(instance.state, "provisioning");
});

Deno.test("create is idempotent by placement label", async () => {
  const { fake, provider } = setup();
  const a = await provider.createInstance({ ...PARAMS, placementId: "PL-1" });
  const b = await provider.createInstance({ ...PARAMS, placementId: "PL-1", idempotencyKey: "other" });
  assertEquals(a.providerResourceId, b.providerResourceId);
  assertEquals(fake.requests.filter((r) => r.method === "POST").length, 1);
});

Deno.test("create for another placement of the same owner makes a new sandbox", async () => {
  // A same-provider resize must never get the source sandbox back as its target.
  const { fake, provider } = setup();
  const source = await provider.createInstance({ ...PARAMS, placementId: "PL-1" });
  const target = await provider.createInstance({ ...PARAMS, placementId: "PL-2", idempotencyKey: "other" });
  assert(target.providerResourceId !== source.providerResourceId);
  assertEquals(defined(fake.sandboxes.get(target.providerResourceId)).labels.treq_placement, "pl-2");
});

for (const failed of ["error", "build_failed"]) {
  Deno.test(`create skips a ${failed} sandbox in the lookup`, async () => {
    const { fake, provider } = setup();
    const a = await provider.createInstance(PARAMS);
    defined(fake.sandboxes.get(a.providerResourceId)).state = failed;
    const b = await provider.createInstance(PARAMS);
    assert(a.providerResourceId !== b.providerResourceId);
  });
}

Deno.test("create skips destroyed sandboxes in the lookup", async () => {
  const { fake, provider } = setup();
  const a = await provider.createInstance(PARAMS);
  defined(fake.sandboxes.get(a.providerResourceId)).state = "destroyed";
  const b = await provider.createInstance(PARAMS);
  assert(a.providerResourceId !== b.providerResourceId);
});

Deno.test("create rejects an unknown machine type", async () => {
  const { provider } = setup();
  const err = await assertRejects(
    () => provider.createInstance({ ...PARAMS, machineType: "huge" }),
    ProviderError,
  );
  assertEquals(err.kind, "invalid_request");
});

Deno.test("start on a started sandbox is a no-op", async () => {
  const { provider } = setup();
  const { providerResourceId: id } = await provider.createInstance(PARAMS);
  await provider.wakeInstance(id);
  await provider.wakeInstance(id);
  assertEquals((await provider.getInstance(id)).state, "ready");
});

Deno.test("exec posts to the toolbox with envs and a timeout", async () => {
  const { fake, provider } = setup();
  const { providerResourceId: id } = await provider.createInstance(PARAMS);
  fake.executeResult = { exitCode: 3, result: "out" };
  const res = await provider.execOnMachine(id, ["echo", "it's"], 30, { FOO: "b r" });
  assertEquals(res, { exitCode: 3, stdout: "out", stderr: "" });
  const req = defined(fake.requests.at(-1));
  assertEquals(req.host, "proxy.daytona.example.invalid");
  assertEquals(req.path, `/toolbox/${id}/process/execute`);
  assertEquals(req.headers.get("Authorization"), "Bearer dtn_test");
  assertEquals(req.body, { command: `'echo' 'it'\\''s'`, envs: { FOO: "b r" }, timeout: 30 });
});

Deno.test("exec rejects bad env names", async () => {
  const { provider } = setup();
  const { providerResourceId: id } = await provider.createInstance(PARAMS);
  const err = await assertRejects(
    () => provider.execOnMachine(id, ["true"], 5, { "bad name": "x" }),
    ProviderError,
  );
  assertEquals(err.kind, "invalid_request");
});

Deno.test("ensureService runs a pidfile supervisor through exec", async () => {
  const { fake, provider } = setup();
  const { providerResourceId: id } = await provider.createInstance(PARAMS);
  await provider.ensureService(id, "sshd", { cmd: "/usr/sbin/sshd", args: ["-D"] });
  const body = defined(fake.requests.at(-1)).body as { command: string };
  assert(body.command.includes("sshd.pid"));
  assert(body.command.includes("/usr/sbin/sshd"));
  fake.executeResult = { exitCode: 1, result: "boom" };
  await assertRejects(
    () => provider.ensureService(id, "sshd", { cmd: "/usr/sbin/sshd", args: [] }),
    ProviderError,
  );
});

Deno.test("gatewayEndpoint returns the gateway port's preview URL with the preview token header", async () => {
  const { fake, provider } = setup();
  const { providerResourceId: id } = await provider.createInstance(PARAMS);
  await provider.stopInstance(id);
  const endpoint = await provider.gatewayEndpoint(id);
  // A stopped sandbox is started first.
  assert(fake.requests.some((r) => r.path === `/api/sandbox/${id}/start`));
  assertEquals(endpoint, {
    url: `wss://8022-${id}.proxy.daytona.example.invalid/`,
    headers: { "x-daytona-preview-token": "preview_tok" },
  });
});

Deno.test("keepAlive refreshes the sandbox's last activity", async () => {
  const { fake, provider } = setup();
  const { providerResourceId: id } = await provider.createInstance(PARAMS);
  await provider.keepAlive(id);
  const req = defined(fake.requests.at(-1));
  assertEquals(req.method, "POST");
  assertEquals(req.path, `/api/sandbox/${id}/last-activity`);
});

Deno.test("getInstance reports the provider's updatedAt as stateChangedAt", async () => {
  const { fake, provider } = setup();
  fake.sandboxes.set("dt_t", {
    id: "dt_t", name: "t", state: "stopped", labels: {}, cpu: 2, memory: 4, disk: 10,
    autoStopInterval: 60, autoArchiveInterval: 0, autoDeleteInterval: -1, toolboxProxyUrl: "",
    updatedAt: "2026-10-28T10:01:00Z",
  } as unknown as Parameters<typeof fake.sandboxes.set>[1]);
  assertEquals((await provider.getInstance("dt_t")).stateChangedAt, "2026-10-28T10:01:00Z");
});

Deno.test("create maps treq regions to Daytona targets", async () => {
  for (const [region, target] of [["us_east", "us"], ["us_west", "us"], ["eu_west", "eu"]] as const) {
    const { fake, provider } = setup();
    const { providerResourceId: id } = await provider.createInstance({ ...PARAMS, region });
    assertEquals(defined(fake.sandboxes.get(id)).target, target);
  }
});

Deno.test("create rejects a region Daytona has no target for", async () => {
  const { provider } = setup();
  const err = await assertRejects(() => provider.createInstance({ ...PARAMS, region: "ap_southeast" }), ProviderError);
  assertEquals(err.kind, "invalid_request");
});

Deno.test("the placement lookup filters states on the server and follows nextCursor", async () => {
  const { fake, provider } = setup();
  const first = await provider.createInstance({ ...PARAMS, placementId: "PL-1" });
  fake.pageSize = 1;
  // A destroyed sandbox with the same labels fills the first page.
  const destroyed = { ...defined(fake.sandboxes.get(first.providerResourceId)), id: "dt_old", state: "destroyed" };
  fake.sandboxes.delete(first.providerResourceId);
  fake.sandboxes.set("dt_old", destroyed);
  fake.sandboxes.set(first.providerResourceId, { ...destroyed, id: first.providerResourceId, state: "started" });
  const again = await provider.createInstance({ ...PARAMS, placementId: "PL-1", idempotencyKey: "other" });
  assertEquals(again.providerResourceId, first.providerResourceId);
  const list = defined(fake.requests.find((r) => r.method === "GET" && r.path === "/api/sandbox"));
  assert(defined(list.search.get("states")).split(",").includes("started"));
  assert(!defined(list.search.get("states")).split(",").includes("destroyed"));
});
