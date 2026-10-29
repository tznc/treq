// E2B adapter request shapes, checked against an in-memory fake of E2B's
// REST API (github.com/e2b-dev/E2B spec/openapi.yml) and envd's process
// service. No network.
//
// Run with: `deno test --allow-env supabase/functions/tests/e2b_adapter.test.ts`

import {
  assert,
  assertEquals,
  assertRejects,
  assertThrows,
} from "https://deno.land/std@0.224.0/assert/mod.ts";
import {
  E2B_CAPABILITIES,
  E2BProvider,
  e2bConfigFromEnv,
  normalizeE2BState,
} from "../_shared/remote/e2b-adapter.ts";
import { type CreateInstanceParams, ProviderError } from "../_shared/remote/provider.ts";
import { FakeE2B } from "./fake_e2b.ts";

const CONFIG = {
  baseUrl: "https://api.e2b.example.invalid/",
  apiKey: "e2b_test",
  template: "treq",
  autoStopMinutes: 60,
};

function setup(apiKey = CONFIG.apiKey) {
  const fake = new FakeE2B();
  const provider = new E2BProvider({ ...CONFIG, apiKey }, { fetch: fake.fetch });
  return { fake, provider };
}

const PARAMS: CreateInstanceParams = {
  ownerUserId: "Owner_1",
  region: "us_east",
  sizePreset: "large",
  manifestVersion: 3,
  idempotencyKey: "Idem_1",
  machineType: "m",
  placementId: "PL-1",
};

function envOf(values: Record<string, string>) {
  return (key: string) => values[key];
}

Deno.test("e2bConfigFromEnv needs the API key, defaults the URL and template", () => {
  const err = assertThrows(() => e2bConfigFromEnv(envOf({})), ProviderError);
  assertEquals(err.kind, "invalid_request");
  assertEquals(e2bConfigFromEnv(envOf({ TREQ_E2B_API_KEY: "k" })), {
    baseUrl: "https://api.e2b.app",
    apiKey: "k",
    template: "treq-sandbox",
    autoStopMinutes: 60,
  });
  const custom = e2bConfigFromEnv(envOf({
    TREQ_E2B_API_KEY: "k",
    TREQ_E2B_API_URL: "https://u",
    TREQ_E2B_TEMPLATE: "t",
    TREQ_PROVIDER_AUTOSTOP_MIN: "90",
  }));
  assertEquals([custom.baseUrl, custom.template, custom.autoStopMinutes], ["https://u", "t", 90]);
});

Deno.test("a BYOS credential replaces treq's key", () => {
  assertEquals(e2bConfigFromEnv(envOf({}), { apiKey: "user" }).apiKey, "user");
  assertEquals(e2bConfigFromEnv(envOf({ TREQ_E2B_API_KEY: "managed" }), { apiKey: "user" }).apiKey, "user");
});

Deno.test("states normalize", () => {
  assertEquals(normalizeE2BState("running"), "ready");
  assertEquals(normalizeE2BState("paused"), "suspended");
  assertEquals(normalizeE2BState("pausing"), "suspended");
  assertEquals(normalizeE2BState("weird"), "degraded");
});

Deno.test("capabilities: per-second billing, pause keeps disk, gateway via ingress", () => {
  assertEquals(E2B_CAPABILITIES.billingGranularitySec, 1);
  assert(E2B_CAPABILITIES.stopKeepsDisk);
  assert(E2B_CAPABILITIES.execApi);
  assert(E2B_CAPABILITIES.tcpProxy);
  assertEquals(E2B_CAPABILITIES.serviceSupervisor, false);
});

Deno.test("create looks up by owner and placement, then POSTs the size's template with autoPause", async () => {
  const { fake, provider } = setup();
  const instance = await provider.createInstance(PARAMS);
  const [lookup, create] = fake.requests;
  assertEquals(lookup.method, "GET");
  assertEquals(lookup.path, "/v2/sandboxes");
  assertEquals(lookup.search.get("metadata"), "treq_owner=owner_1&treq_placement=pl-1");
  assertEquals(lookup.search.get("state"), "running,paused");
  assertEquals(create.method, "POST");
  assertEquals(create.path, "/v2/sandboxes");
  assertEquals(create.headers.get("X-API-Key"), "e2b_test");
  assertEquals(create.headers.get("Idempotency-Key"), "idem-1");
  assertEquals(create.body, {
    templateID: "treq-m",
    timeout: 3600,
    autoPause: true,
    metadata: { treq_owner: "owner_1", treq_placement: "pl-1", treq_manifest: "3" },
    network: { allowPublicTraffic: false },
  });
  assertEquals(instance.providerResourceId, "sbx1");
  assertEquals(instance.state, "ready");
});

Deno.test("create is idempotent by placement", async () => {
  const { fake, provider } = setup();
  const a = await provider.createInstance(PARAMS);
  const b = await provider.createInstance({ ...PARAMS, idempotencyKey: "other" });
  assertEquals(a.providerResourceId, b.providerResourceId);
  assertEquals(fake.requests.filter((r) => r.method === "POST").length, 1);
});

Deno.test("create for another placement of the same owner makes a new sandbox", async () => {
  const { fake, provider } = setup();
  const source = await provider.createInstance(PARAMS);
  const target = await provider.createInstance({ ...PARAMS, placementId: "PL-2", idempotencyKey: "other" });
  assert(target.providerResourceId !== source.providerResourceId);
  assertEquals(fake.sandboxes.get(target.providerResourceId)!.metadata.treq_placement, "pl-2");
});

Deno.test("create without a placement id uses the idempotency key as the placement", async () => {
  const { fake, provider } = setup();
  await provider.createInstance({ ...PARAMS, placementId: undefined });
  assertEquals(fake.requests[0].search.get("metadata"), "treq_owner=owner_1&treq_placement=idem_1");
});

Deno.test("create skips a killed sandbox (gone from the running/paused list)", async () => {
  const { fake, provider } = setup();
  const a = await provider.createInstance(PARAMS);
  fake.sandboxes.delete(a.providerResourceId);
  const b = await provider.createInstance({ ...PARAMS, idempotencyKey: "retry" });
  assert(a.providerResourceId !== b.providerResourceId);
});

Deno.test("create rejects an unknown machine type", async () => {
  const { provider } = setup();
  const err = await assertRejects(() => provider.createInstance({ ...PARAMS, machineType: "4x8192" }), ProviderError);
  assertEquals(err.kind, "invalid_request");
});

Deno.test("stop pauses; pausing twice and stopping a missing sandbox succeed", async () => {
  const { provider } = setup();
  const { providerResourceId: id } = await provider.createInstance(PARAMS);
  await provider.stopInstance(id);
  await provider.stopInstance(id);
  await provider.stopInstance("missing");
  assertEquals((await provider.getInstance(id)).state, "suspended");
});

Deno.test("wake connects with the backstop timeout", async () => {
  const { fake, provider } = setup();
  const { providerResourceId: id } = await provider.createInstance(PARAMS);
  await provider.stopInstance(id);
  await provider.wakeInstance(id);
  const req = fake.requests.at(-1)!;
  assertEquals(req.path, `/v2/sandboxes/${id}/connect`);
  assertEquals(req.body, { timeout: 3600 });
  assertEquals((await provider.getInstance(id)).state, "ready");
});

Deno.test("keepAlive resets the sandbox timeout to the backstop", async () => {
  const { fake, provider } = setup();
  const { providerResourceId: id } = await provider.createInstance(PARAMS);
  fake.sandboxes.get(id)!.timeout = 5;
  await provider.keepAlive(id);
  const req = fake.requests.at(-1)!;
  assertEquals([req.method, req.path, req.body], ["POST", `/sandboxes/${id}/timeout`, { timeout: 3600 }]);
  assertEquals(fake.sandboxes.get(id)!.timeout, 3600);
});

Deno.test("exec runs argv and env through envd with the access token", async () => {
  const { fake, provider } = setup();
  const { providerResourceId: id } = await provider.createInstance(PARAMS);
  fake.commandResult = { exitCode: 3, stdout: "out", stderr: "err" };
  const res = await provider.execOnMachine(id, ["echo", "hi"], 10, { FOO: "bar" });
  assertEquals(res, { exitCode: 3, stdout: "out", stderr: "err" });
  const req = fake.requests.at(-1)!;
  assertEquals(req.host, `49983-${id}.e2b.app`);
  assertEquals(req.path, "/process.Process/Start");
  assertEquals(req.headers.get("X-Access-Token"), `envd-${id}`);
  assertEquals(req.body, { process: { cmd: "echo", args: ["hi"], envs: { FOO: "bar" } } });
});

Deno.test("ensureService runs a pidfile supervisor and fails when it fails", async () => {
  const { fake, provider } = setup();
  const { providerResourceId: id } = await provider.createInstance(PARAMS);
  await provider.ensureService(id, "sshd", { cmd: "/usr/sbin/sshd", args: ["-D"] });
  const script = (fake.requests.at(-1)!.body as { process: { args: string[] } }).process.args[1];
  assert(script.includes("sshd.pid"));
  fake.commandResult = { exitCode: 1, stdout: "", stderr: "boom" };
  await assertRejects(() => provider.ensureService(id, "sshd", { cmd: "/usr/sbin/sshd", args: [] }), ProviderError);
});

Deno.test("gatewayEndpoint resumes the sandbox and names the gateway port with the traffic token", async () => {
  const { fake, provider } = setup();
  const { providerResourceId: id } = await provider.createInstance(PARAMS);
  await provider.stopInstance(id);
  const endpoint = await provider.gatewayEndpoint(id);
  assert(fake.requests.some((r) => r.path === `/v2/sandboxes/${id}/connect`));
  assertEquals(endpoint, {
    url: `wss://8022-${id}.e2b.app/`,
    headers: { "e2b-traffic-access-token": `traffic-${id}` },
  });
});

Deno.test("validateCredentials accepts a good key and maps 401 to unauthorized", async () => {
  await setup().provider.validateCredentials();
  const err = await assertRejects(() => setup("wrong").provider.validateCredentials(), ProviderError);
  assertEquals(err.kind, "unauthorized");
});
