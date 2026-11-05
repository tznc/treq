// Cloudflare Sandboxes adapter request shapes, checked against an in-memory
// fake of treq's sandbox bridge Worker (cloudflare/sandbox-bridge). No network.
//
// Run with: `deno test --allow-env supabase/functions/tests/cloudflare_adapter.test.ts`

import { defined } from "./remote_fakes.ts";
import { assert, assertEquals, assertRejects, assertThrows } from "https://deno.land/std@0.224.0/assert/mod.ts";
import {
  CLOUDFLARE_CAPABILITIES,
  CloudflareProvider,
  cloudflareConfigFromEnv,
  normalizeCloudflareState,
  parseCloudflareCredential,
} from "../_shared/remote/cloudflare-adapter.ts";
import { type CreateInstanceParams, ProviderError } from "../_shared/remote/provider.ts";
import { FakeCloudflare } from "./fake_cloudflare.ts";

const CONFIG = {
  bridgeUrl: "https://treq-sandbox.acme.example.invalid/",
  token: "cf_test",
  autoStopMinutes: 60,
};

function setup(token = CONFIG.token) {
  const fake = new FakeCloudflare();
  const provider = new CloudflareProvider({ ...CONFIG, token }, { fetch: fake.fetch });
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

Deno.test("a credential is the bridge URL with the token as its fragment", () => {
  assertEquals(parseCloudflareCredential("https://treq-sandbox.acme.workers.dev#s3cret"), {
    bridgeUrl: "https://treq-sandbox.acme.workers.dev",
    token: "s3cret",
  });
  for (const bad of ["s3cret", "http://insecure.dev#t", "https://no-token.dev", "https://x.dev#"]) {
    const err = assertThrows(() => parseCloudflareCredential(bad), ProviderError);
    assertEquals(err.kind, "invalid_request");
  }
});

Deno.test("cloudflareConfigFromEnv needs the bridge URL and token; BYOS replaces both", () => {
  const err = assertThrows(() => cloudflareConfigFromEnv(envOf({})), ProviderError);
  assertEquals(err.kind, "invalid_request");
  assertEquals(
    cloudflareConfigFromEnv(envOf({ TREQ_CLOUDFLARE_BRIDGE_URL: "https://b", TREQ_CLOUDFLARE_BRIDGE_TOKEN: "t" })),
    { bridgeUrl: "https://b", token: "t", autoStopMinutes: 60 },
  );
  const byos = cloudflareConfigFromEnv(envOf({ TREQ_CLOUDFLARE_BRIDGE_TOKEN: "managed" }), {
    apiKey: "https://user.workers.dev#mine",
  });
  assertEquals([byos.bridgeUrl, byos.token], ["https://user.workers.dev", "mine"]);
});

Deno.test("states normalize", () => {
  assertEquals(normalizeCloudflareState("running"), "ready");
  assertEquals(normalizeCloudflareState("starting"), "waking");
  assertEquals(normalizeCloudflareState("sleeping"), "suspended");
  assertEquals(normalizeCloudflareState("stopped"), "suspended");
  assertEquals(normalizeCloudflareState("weird"), "degraded");
});

Deno.test("capabilities: per-10ms billing rounds to 1 s, sleep drops the disk, gateway via the bridge", () => {
  assertEquals(CLOUDFLARE_CAPABILITIES.billingGranularitySec, 1);
  assertEquals(CLOUDFLARE_CAPABILITIES.stopKeepsDisk, false);
  assert(CLOUDFLARE_CAPABILITIES.execApi);
  assert(CLOUDFLARE_CAPABILITIES.tcpProxy);
  assertEquals(CLOUDFLARE_CAPABILITIES.serviceSupervisor, false);
});

Deno.test("create PUTs a sandbox named after the placement, with the tier's instance type", async () => {
  const { fake, provider } = setup();
  const instance = await provider.createInstance(PARAMS);
  const [req] = fake.requests;
  assertEquals([req.method, req.host, req.path], ["PUT", "treq-sandbox.acme.example.invalid", "/v1/sandboxes/treq-pl-1"]);
  assertEquals(req.headers.get("Authorization"), "Bearer cf_test");
  assertEquals(req.body, {
    instanceType: "m",
    sleepAfterSec: 3600,
    labels: { treq_owner: "owner_1", treq_placement: "pl-1", treq_manifest: "3" },
  });
  assertEquals(instance.providerResourceId, "treq-pl-1");
  assertEquals(instance.state, "ready");
});

Deno.test("create is idempotent by placement; another placement gets another sandbox", async () => {
  const { fake, provider } = setup();
  const a = await provider.createInstance(PARAMS);
  const b = await provider.createInstance({ ...PARAMS, idempotencyKey: "other" });
  const c = await provider.createInstance({ ...PARAMS, placementId: "PL-2" });
  assertEquals(a.providerResourceId, b.providerResourceId);
  assert(c.providerResourceId !== a.providerResourceId);
  assertEquals(fake.sandboxes.size, 2);
});

Deno.test("create without a placement id names the sandbox after the idempotency key", async () => {
  const { provider } = setup();
  assertEquals((await provider.createInstance({ ...PARAMS, placementId: undefined })).providerResourceId, "treq-idem_1");
});

Deno.test("create rejects l/xl: Cloudflare instances top out at 4 vCPU", async () => {
  const { provider } = setup();
  for (const machineType of ["l", "xl", "standard-4"]) {
    const err = await assertRejects(() => provider.createInstance({ ...PARAMS, machineType }), ProviderError);
    assertEquals(err.kind, "invalid_request");
  }
});

Deno.test("stop sleeps the sandbox; stopping a missing one succeeds", async () => {
  const { provider } = setup();
  const { providerResourceId: id } = await provider.createInstance(PARAMS);
  await provider.stopInstance(id);
  await provider.stopInstance("missing");
  assertEquals((await provider.getInstance(id)).state, "suspended");
});

Deno.test("wake starts the sandbox", async () => {
  const { fake, provider } = setup();
  const { providerResourceId: id } = await provider.createInstance(PARAMS);
  await provider.stopInstance(id);
  await provider.wakeInstance(id);
  assertEquals(defined(fake.requests.at(-1)).path, `/v1/sandboxes/${id}/start`);
  assertEquals((await provider.getInstance(id)).state, "ready");
});

Deno.test("keepAlive resets sleepAfter to the backstop", async () => {
  const { fake, provider } = setup();
  const { providerResourceId: id } = await provider.createInstance(PARAMS);
  defined(fake.sandboxes.get(id)).sleepAfterSec = 5;
  await provider.keepAlive(id);
  const req = defined(fake.requests.at(-1));
  assertEquals([req.method, req.path, req.body], ["POST", `/v1/sandboxes/${id}/keepalive`, { sleepAfterSec: 3600 }]);
  assertEquals(defined(fake.sandboxes.get(id)).sleepAfterSec, 3600);
});

Deno.test("delete is idempotent", async () => {
  const { provider } = setup();
  const { providerResourceId: id } = await provider.createInstance(PARAMS);
  await provider.deleteInstance(id);
  await provider.deleteInstance(id);
  const err = await assertRejects(() => provider.getInstance(id), ProviderError);
  assertEquals(err.kind, "not_found");
});

Deno.test("exec sends argv, env and the timeout to the bridge", async () => {
  const { fake, provider } = setup();
  const { providerResourceId: id } = await provider.createInstance(PARAMS);
  fake.commandResult = { exitCode: 3, stdout: "out", stderr: "err" };
  const res = await provider.execOnMachine(id, ["echo", "hi"], 10, { FOO: "bar" });
  assertEquals(res, { exitCode: 3, stdout: "out", stderr: "err" });
  const req = defined(fake.requests.at(-1));
  assertEquals([req.method, req.path], ["POST", `/v1/sandboxes/${id}/exec`]);
  assertEquals(req.body, { argv: ["echo", "hi"], env: { FOO: "bar" }, timeoutMs: 10_000 });
});

Deno.test("exec rejects an empty command and bad env names before calling out", async () => {
  const { fake, provider } = setup();
  await assertRejects(() => provider.execOnMachine("x", []), ProviderError);
  await assertRejects(() => provider.execOnMachine("x", ["true"], 5, { "A-B": "1" }), ProviderError);
  assertEquals(fake.requests.length, 0);
});

Deno.test("ensureService runs a pidfile supervisor and fails when it fails", async () => {
  const { fake, provider } = setup();
  const { providerResourceId: id } = await provider.createInstance(PARAMS);
  await provider.ensureService(id, "sshd", { cmd: "/usr/sbin/sshd", args: ["-D"] });
  const argv = (defined(fake.requests.at(-1)).body as { argv: string[] }).argv;
  assert(argv[2].includes("sshd.pid"));
  fake.commandResult = { exitCode: 1, stdout: "", stderr: "boom" };
  await assertRejects(() => provider.ensureService(id, "sshd", { cmd: "/usr/sbin/sshd", args: [] }), ProviderError);
});

Deno.test("gatewayEndpoint starts the sandbox and names the bridge's port route with the token", async () => {
  const { fake, provider } = setup();
  const { providerResourceId: id } = await provider.createInstance(PARAMS);
  await provider.stopInstance(id);
  const endpoint = await provider.gatewayEndpoint(id);
  assert(fake.requests.some((r) => r.path === `/v1/sandboxes/${id}/start`));
  assertEquals(endpoint, {
    url: `wss://treq-sandbox.acme.example.invalid/v1/sandboxes/${id}/ports/8022`,
    headers: { Authorization: "Bearer cf_test" },
  });
});

Deno.test("validateCredentials accepts a good token and maps 401 to unauthorized", async () => {
  await setup().provider.validateCredentials();
  const err = await assertRejects(() => setup("wrong").provider.validateCredentials(), ProviderError);
  assertEquals(err.kind, "unauthorized");
});
