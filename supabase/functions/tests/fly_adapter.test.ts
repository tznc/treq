// Fly Machines adapter request shapes, checked against an in-memory fake of
// the Machines REST API (docs.fly.io/machines/api). No network.
//
// Run with: `deno test --allow-env supabase/functions/tests/fly_adapter.test.ts`

import { assert, assertEquals, assertRejects, assertThrows } from "https://deno.land/std@0.224.0/assert/mod.ts";
import {
  FLY_CAPABILITIES,
  FLY_GATEWAY_LISTEN,
  flyConfigFromEnv,
  FlyMachinesProvider,
  normalizeFlyState,
} from "../_shared/remote/fly-adapter.ts";
import { type CreateInstanceParams, ProviderError } from "../_shared/remote/provider.ts";
import { FakeFly } from "./fake_fly.ts";

const CONFIG = {
  baseUrl: "https://fly.example.invalid/",
  apiToken: "fly_test",
  app: "treq-sandboxes",
  image: "registry.fly.io/treq-dev:1",
};

function setup() {
  const fake = new FakeFly();
  const sleeps: number[] = [];
  const provider = new FlyMachinesProvider(CONFIG, {
    fetch: fake.fetch,
    sleep: (ms) => {
      sleeps.push(ms);
      return Promise.resolve();
    },
  });
  return { fake, provider, sleeps };
}

const PARAMS: CreateInstanceParams = {
  ownerUserId: "owner-1",
  region: "eu_west",
  sizePreset: "large",
  manifestVersion: 2,
  idempotencyKey: "idem-1",
  machineType: "performance-4x:8192",
  placementId: "PL-1",
};

Deno.test("flyConfigFromEnv needs token, app and image, and defaults the URL", () => {
  assertThrows(() => flyConfigFromEnv(() => undefined), ProviderError);
  const env: Record<string, string> = { TREQ_FLY_API_TOKEN: "t", TREQ_FLY_APP: "a", TREQ_FLY_IMAGE: "i" };
  assertEquals(flyConfigFromEnv((k) => env[k]), {
    baseUrl: "https://api.machines.dev",
    apiToken: "t",
    app: "a",
    image: "i",
  });
});

Deno.test("normalizeFlyState maps vendor states", () => {
  assertEquals(normalizeFlyState("started"), "ready");
  assertEquals(normalizeFlyState("created"), "provisioning");
  assertEquals(normalizeFlyState("starting"), "waking");
  assertEquals(normalizeFlyState("stopped"), "suspended");
  assertEquals(normalizeFlyState("suspended"), "suspended");
  assertEquals(normalizeFlyState("destroyed"), "deleted");
  assertEquals(normalizeFlyState("failed"), "failed");
  assertEquals(normalizeFlyState("weird"), "degraded");
});

Deno.test("create makes a volume, then a machine mounting it with the gateway service", async () => {
  const { fake, provider } = setup();
  const inst = await provider.createInstance(PARAMS);
  const posts = fake.requests.filter((r) => r.method === "POST");
  assertEquals(posts.map((r) => r.path), ["/v1/apps/treq-sandboxes/volumes", "/v1/apps/treq-sandboxes/machines"]);
  assertEquals(fake.requests[0].headers.get("authorization"), "Bearer fly_test");
  const vol = posts[0].body as any;
  assertEquals(vol.region, "lhr");
  assertEquals(vol.size_gb, 40);
  const m = fake.machines.get(inst.providerResourceId)!;
  assertEquals(m.region, "lhr");
  assertEquals(m.config.image, CONFIG.image);
  assertEquals(m.config.guest, { cpu_kind: "performance", cpus: 4, memory_mb: 8192 });
  assertEquals(m.config.mounts?.[0].path, "/home");
  assert(fake.volumes.get(m.config.mounts![0].volume));
  const svc = m.config.services![0] as any;
  assertEquals(svc.internal_port, 8022);
  assertEquals(svc.autostart, false);
  assertEquals(svc.autostop, "stop");
  assertEquals(svc.ports, [{ port: 443, handlers: ["tls", "http"] }]);
  assertEquals(m.config.metadata?.treq_placement, "pl-1");
});

Deno.test("create is idempotent by machine name", async () => {
  const { fake, provider } = setup();
  const a = await provider.createInstance(PARAMS);
  const b = await provider.createInstance(PARAMS);
  assertEquals(a.providerResourceId, b.providerResourceId);
  assertEquals(fake.machines.size, 1);
  assertEquals(fake.volumes.size, 1);
});

Deno.test("create reuses an orphaned volume left by a failed create", async () => {
  const { fake, provider } = setup();
  await provider.createInstance(PARAMS);
  const [m] = fake.machines.values();
  fake.machines.delete(m.id);
  for (const v of fake.volumes.values()) v.attached_machine_id = null;
  await provider.createInstance(PARAMS);
  assertEquals(fake.volumes.size, 1);
});

Deno.test("an unknown machine type is rejected", async () => {
  const { provider } = setup();
  const err = await assertRejects(() => provider.createInstance({ ...PARAMS, machineType: "huge" }), ProviderError);
  assertEquals(err.kind, "invalid_request");
});

Deno.test("delete removes the machine and its volume; repeat is ok", async () => {
  const { fake, provider } = setup();
  const { providerResourceId: id } = await provider.createInstance(PARAMS);
  await provider.deleteInstance(id);
  assertEquals(fake.machines.size, 0);
  assertEquals(fake.volumes.size, 0);
  await provider.deleteInstance(id);
});

Deno.test("delete waits for the volume to detach, with a bounded backoff", async () => {
  const { fake, provider, sleeps } = setup();
  const { providerResourceId: id } = await provider.createInstance(PARAMS);
  fake.detachDelay = 2;
  await provider.deleteInstance(id);
  assertEquals(fake.machines.size, 0);
  assertEquals(fake.volumes.size, 0);
  assertEquals(sleeps.length, 2);
});

Deno.test("a retried delete after the machine is gone still removes the placement's volume", async () => {
  const { fake, provider } = setup();
  const { providerResourceId: id } = await provider.createInstance(PARAMS);
  // The first delete destroyed the machine but its volume never detached in time.
  fake.machines.delete(id);
  for (const v of fake.volumes.values()) v.attached_machine_id = null;
  await provider.deleteInstance(id, { placementId: PARAMS.placementId });
  assertEquals(fake.volumes.size, 0);
});

Deno.test("delete leaves other placements' volumes alone", async () => {
  const { fake, provider } = setup();
  const a = await provider.createInstance(PARAMS);
  await provider.createInstance({ ...PARAMS, placementId: "PL-2", idempotencyKey: "idem-2" });
  await provider.deleteInstance(a.providerResourceId, { placementId: PARAMS.placementId });
  assertEquals(fake.machines.size, 1);
  assertEquals(fake.volumes.size, 1);
});

Deno.test("stop keeps the volume", async () => {
  const { fake, provider } = setup();
  const { providerResourceId: id } = await provider.createInstance(PARAMS);
  await provider.stopInstance(id);
  assertEquals(fake.volumes.size, 1);
});

Deno.test("exec posts the command and returns the result", async () => {
  const { fake, provider } = setup();
  const { providerResourceId: id } = await provider.createInstance(PARAMS);
  await provider.wakeInstance(id);
  fake.execResult = { exit_code: 3, stdout: "out", stderr: "err" };
  const out = await provider.execOnMachine(id, ["echo", "hi"], 5, { A: "b" });
  assertEquals(out, { exitCode: 3, stdout: "out", stderr: "err" });
  const req = fake.requests.at(-1)!;
  assertEquals(req.path, `/v1/apps/treq-sandboxes/machines/${id}/exec`);
  assertEquals((req.body as any).command, ["env", "A=b", "echo", "hi"]);
  assertEquals((req.body as any).timeout, 5);
});

Deno.test("gatewayEndpoint routes to the machine by fly-force-instance-id", async () => {
  const { provider } = setup();
  const { providerResourceId: id } = await provider.createInstance(PARAMS);
  assertEquals(await provider.gatewayEndpoint(id), {
    url: "wss://treq-sandboxes.fly.dev/",
    headers: { "fly-force-instance-id": id },
  });
});

Deno.test("gatewayEndpoint wakes a stopped machine (autostart is off)", async () => {
  const { fake, provider } = setup();
  const { providerResourceId: id } = await provider.createInstance(PARAMS);
  await provider.stopInstance(id);
  await provider.gatewayEndpoint(id);
  assertEquals(fake.machines.get(id)!.state, "started");
});

Deno.test("the gateway listens on all interfaces so Fly Proxy can reach it", () => {
  const { provider } = setup();
  assertEquals(provider.gatewayListenAddress, FLY_GATEWAY_LISTEN);
  assertEquals(FLY_GATEWAY_LISTEN, "0.0.0.0:8022");
});

Deno.test("capabilities", () => {
  assertEquals(FLY_CAPABILITIES, {
    stopKeepsDisk: true,
    execApi: true,
    serviceSupervisor: false,
    tcpProxy: true,
    billingGranularitySec: 1,
  });
});

Deno.test("getInstance reports the machine's updated_at as stateChangedAt", async () => {
  const { fake, provider } = setup();
  fake.machines.set("m_t", {
    id: "m_t", name: "t", state: "stopped", region: "ams", config: {},
    updated_at: "2026-10-28T10:01:00Z",
  } as unknown as Parameters<typeof fake.machines.set>[1]);
  assertEquals((await provider.getInstance("m_t")).stateChangedAt, "2026-10-28T10:01:00Z");
});
