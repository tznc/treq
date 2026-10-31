// Boat adapter request shapes, checked against an in-memory fake of Boat's
// public API (docs.boat.dev/openapi/boat-v1.yaml). No network.
//
// Run with: `deno test --allow-env supabase/functions/tests/boat_adapter.test.ts`

import { defined } from "./remote_fakes.ts";
import {
  assert,
  assertEquals,
  assertRejects,
  assertThrows,
} from "https://deno.land/std@0.224.0/assert/mod.ts";
import {
  BOAT_CAPABILITIES,
  BoatProvider,
  boatConfigFromEnv,
  normalizeBoatState,
} from "../_shared/remote/boat-adapter.ts";
import { shellQuote } from "../_shared/remote/provider-http.ts";
import { ProviderError, type CreateInstanceParams } from "../_shared/remote/provider.ts";
import { FakeBoat } from "./fake_boat.ts";

const CONFIG = { baseUrl: "https://boat.example.invalid/api/v1/", apiToken: "boat_test" };

function setup() {
  const fake = new FakeBoat();
  const provider = new BoatProvider(CONFIG, { fetch: fake.fetch });
  return { fake, provider };
}

const PARAMS: CreateInstanceParams = {
  ownerUserId: "Owner_1",
  region: "us_east",
  sizePreset: "large",
  manifestVersion: 3,
  idempotencyKey: "idem-1",
  machineType: "large",
};

Deno.test("boatConfigFromEnv needs both secrets", () => {
  const err = assertThrows(() => boatConfigFromEnv(() => undefined), ProviderError);
  assertEquals(err.kind, "invalid_request");
  assertEquals(
    boatConfigFromEnv((k) => ({ TREQ_BOAT_API_URL: "https://u", TREQ_BOAT_API_TOKEN: "t" })[k]),
    { baseUrl: "https://u", apiToken: "t", autoStopMinutes: 60 },
  );
});

Deno.test("boatConfigFromEnv reads the provider auto-stop backstop in minutes", () => {
  const env = (extra: Record<string, string>) => (k: string) =>
    ({ TREQ_BOAT_API_URL: "https://u", TREQ_BOAT_API_TOKEN: "t", ...extra })[k];
  assertEquals(boatConfigFromEnv(env({ TREQ_PROVIDER_AUTOSTOP_MIN: "90" })).autoStopMinutes, 90);
  assertEquals(boatConfigFromEnv(env({ TREQ_PROVIDER_AUTOSTOP_MIN: "0" })).autoStopMinutes, 60);
  assertEquals(boatConfigFromEnv(env({ TREQ_PROVIDER_AUTOSTOP_MIN: "soon" })).autoStopMinutes, 60);
});

Deno.test("create looks up by owner and placement labels, then POSTs with the size and idempotency key", async () => {
  const { fake, provider } = setup();
  const instance = await provider.createInstance(PARAMS);
  const [lookup, create] = fake.requests;
  assertEquals(lookup.method, "GET");
  assertEquals(lookup.path, "/sandboxes");
  assertEquals(lookup.search.getAll("label"), ["treq_owner=owner_1", "treq_placement=idem-1"]);
  assertEquals(create.method, "POST");
  assertEquals(create.path, "/sandboxes");
  assertEquals(create.headers.get("Authorization"), "Bearer boat_test");
  assertEquals(create.headers.get("Idempotency-Key"), "idem-1");
  assertEquals(create.body, {
    type: "large",
    // The provider auto-stop backstop: 60 minutes, longer than our idle timeout.
    ttlSeconds: 3600,
    labels: { treq_owner: "owner_1", treq_placement: "idem-1", treq_manifest: "3" },
  });
  assertEquals(instance.state, "provisioning");
  assertEquals(instance.providerResourceId, "bx_1");
});

Deno.test("create without a machine type uses Boat's default size", async () => {
  const { fake, provider } = setup();
  await provider.createInstance({ ...PARAMS, machineType: undefined });
  assertEquals((fake.requests[1].body as { type: string }).type, "default");
});

Deno.test("create returns the existing sandbox for the same placement under a new key", async () => {
  const { fake, provider } = setup();
  const first = await provider.createInstance({ ...PARAMS, placementId: "PL-1" });
  const second = await provider.createInstance({ ...PARAMS, placementId: "PL-1", idempotencyKey: "idem-2" });
  assertEquals(second.providerResourceId, first.providerResourceId);
  assertEquals(fake.sandboxes.size, 1);
});

Deno.test("create for another placement of the same owner makes a new sandbox", async () => {
  // A same-provider resize must never get the source sandbox back as its target.
  const { fake, provider } = setup();
  const source = await provider.createInstance({ ...PARAMS, placementId: "PL-1" });
  const target = await provider.createInstance({ ...PARAMS, placementId: "PL-2", idempotencyKey: "idem-2" });
  assert(target.providerResourceId !== source.providerResourceId);
  assertEquals(fake.sandboxes.size, 2);
  assertEquals(defined(fake.sandboxes.get(target.providerResourceId)).labels.treq_placement, "pl-2");
});

Deno.test("create skips a failed sandbox in the lookup", async () => {
  const { fake, provider } = setup();
  const first = await provider.createInstance({ ...PARAMS, placementId: "PL-1" });
  defined(fake.sandboxes.get(first.providerResourceId)).state = "error";
  const second = await provider.createInstance({ ...PARAMS, placementId: "PL-1", idempotencyKey: "idem-2" });
  assert(second.providerResourceId !== first.providerResourceId);
  assertEquals(second.state, "provisioning");
});

Deno.test("Boat states normalize to managed states", () => {
  const table: Record<string, string> = {
    init: "provisioning",
    provisioning: "provisioning",
    provisioned: "provisioning",
    cloning: "provisioning",
    ready: "ready",
    idle: "ready",
    running: "ready",
    archiving: "suspended",
    archived: "suspended",
    error: "failed",
    cancelled: "deleted",
    something_new: "degraded",
  };
  for (const [vendor, managed] of Object.entries(table)) {
    assertEquals(normalizeBoatState(vendor), managed, vendor);
  }
});

Deno.test("exec sends one shell-quoted command with env and timeout", async () => {
  const { fake, provider } = setup();
  const { providerResourceId: id } = await provider.createInstance(PARAMS);
  fake.commandResult = { exitCode: 2, stdout: "out", stderr: "err" };
  const result = await provider.execOnMachine(id, ["echo", "it's here"], 45, { FOO: "a b" });
  assertEquals(result, { exitCode: 2, stdout: "out", stderr: "err" });
  const req = defined(fake.requests.at(-1));
  assertEquals(req.path, `/sandboxes/${id}/commands`);
  assertEquals(req.body, {
    command: "env FOO='a b' 'echo' 'it'\\''s here'",
    timeoutSeconds: 45,
  });
});

Deno.test("exec rejects bad env names and clamps the timeout to Boat's range", async () => {
  const { fake, provider } = setup();
  const { providerResourceId: id } = await provider.createInstance(PARAMS);
  await assertRejects(() => provider.execOnMachine(id, ["true"], 5, { "bad-name": "x" }), ProviderError);
  await provider.execOnMachine(id, ["true"], 5000);
  assertEquals((defined(fake.requests.at(-1)).body as { timeoutSeconds: number }).timeoutSeconds, 600);
});

Deno.test("delete sends the confirm header and tolerates 404", async () => {
  const { fake, provider } = setup();
  const { providerResourceId: id } = await provider.createInstance(PARAMS);
  await provider.deleteInstance(id);
  const req = defined(fake.requests.at(-1));
  assertEquals(req.method, "DELETE");
  assertEquals(req.headers.get("X-Ascii-Confirm-Delete"), id);
  await provider.deleteInstance(id);
});

Deno.test("wake resumes a stopped sandbox and is a no-op when it already runs", async () => {
  const { fake, provider } = setup();
  const { providerResourceId: id } = await provider.createInstance(PARAMS);
  await provider.wakeInstance(id); // 409 from a running sandbox is fine
  await provider.stopInstance(id);
  assertEquals(defined(fake.requests.at(-1)).path, `/sandboxes/${id}/stop`);
  await provider.wakeInstance(id);
  assertEquals(defined(fake.requests.at(-1)).path, `/sandboxes/${id}/resume`);
  assertEquals(defined(fake.sandboxes.get(id)).state, "ready");
});

Deno.test("ensureService runs a pidfile supervisor through exec", async () => {
  const { fake, provider } = setup();
  const { providerResourceId: id } = await provider.createInstance(PARAMS);
  await provider.ensureService(id, "sshd", { cmd: "/usr/sbin/sshd", args: ["-D", "-e"] });
  const req = defined(fake.requests.at(-1));
  assertEquals(req.path, `/sandboxes/${id}/commands`);
  const command = (req.body as { command: string }).command;
  assert(command.includes("/var/run/treq/sshd.pid"), command);
  assert(command.includes("nohup"), command);
  assert(command.includes("/usr/sbin/sshd") && command.includes("-D"), command);
});

Deno.test("ensureService rejects names that are not safe file names", async () => {
  const { provider } = setup();
  const err = await assertRejects(
    () => provider.ensureService("bx_1", "../x", { cmd: "true", args: [] }),
    ProviderError,
  );
  assertEquals(err.kind, "invalid_request");
});

Deno.test("ensureService surfaces a failed supervisor start", async () => {
  const { fake, provider } = setup();
  const { providerResourceId: id } = await provider.createInstance(PARAMS);
  fake.commandResult = { exitCode: 1, stdout: "", stderr: "boom" };
  const err = await assertRejects(
    () => provider.ensureService(id, "sshd", { cmd: "/usr/sbin/sshd", args: [] }),
    ProviderError,
  );
  assertEquals(err.kind, "other");
});

Deno.test("gatewayEndpoint sends the port token as a _port_auth cookie, not a redirecting query", async () => {
  const { fake, provider } = setup();
  const { providerResourceId: id } = await provider.createInstance(PARAMS);
  const endpoint = await provider.gatewayEndpoint(id);
  const host = defined(fake.requests.find((r) => r.path === `/sandboxes/${id}/host`));
  assertEquals(host.method, "POST");
  assertEquals(host.body, { port: 8022, public: false, localhost: true });
  assertEquals(endpoint, {
    url: `wss://${id}-8022.boat.example.invalid/`,
    headers: { Cookie: "_port_auth=host_tok" },
  });
});

Deno.test("gatewayEndpoint resumes a stopped sandbox, then re-hosts", async () => {
  const { fake, provider } = setup();
  const { providerResourceId: id } = await provider.createInstance(PARAMS);
  await provider.stopInstance(id);
  await provider.gatewayEndpoint(id);
  const paths = fake.requests.map((r) => r.path);
  assert(paths.indexOf(`/sandboxes/${id}/resume`) < paths.indexOf(`/sandboxes/${id}/host`));
  assert(paths.includes(`/sandboxes/${id}/resume`));
});

for (const code of ["resume_failed", "snapshot_data_expired"]) {
  Deno.test(`wake surfaces a ${code} 409 instead of treating it as awake`, async () => {
    const { fake, provider } = setup();
    const { providerResourceId: id } = await provider.createInstance(PARAMS);
    fake.resumeErrorCode = code;
    const err = await assertRejects(() => provider.wakeInstance(id), ProviderError);
    assertEquals(err.kind, "other");
  });
}

Deno.test("wake retries later while the sandbox is still stopping", async () => {
  const { fake, provider } = setup();
  const { providerResourceId: id } = await provider.createInstance(PARAMS);
  fake.resumeErrorCode = "sandbox_stopping";
  const err = await assertRejects(() => provider.wakeInstance(id), ProviderError);
  assertEquals(err.kind, "unavailable");
});

Deno.test("state changes are timed by stoppedAt, falling back to updatedAt", async () => {
  const { fake, provider } = setup();
  const { providerResourceId: id } = await provider.createInstance(PARAMS);
  const sandbox = defined(fake.sandboxes.get(id));
  sandbox.updatedAt = "2026-01-01T00:00:00Z";
  assertEquals((await provider.getInstance(id)).stateChangedAt, "2026-01-01T00:00:00Z");
  sandbox.stoppedAt = "2026-01-01T00:05:00Z";
  sandbox.updatedAt = "2026-01-01T00:09:00Z";
  assertEquals((await provider.getInstance(id)).stateChangedAt, "2026-01-01T00:05:00Z");
});

Deno.test("shellQuote wraps every argument in single quotes", () => {
  assertEquals(shellQuote(["a", "b c", "it's"]), "'a' 'b c' 'it'\\''s'");
});

Deno.test("Boat capabilities", () => {
  assertEquals(BOAT_CAPABILITIES, {
    stopKeepsDisk: true,
    execApi: true,
    serviceSupervisor: false,
    tcpProxy: true,
    billingGranularitySec: 1,
  });
});

Deno.test("wake resumes with the auto-stop TTL so older sandboxes get the backstop", async () => {
  const { fake, provider } = setup();
  const { providerResourceId: id } = await provider.createInstance(PARAMS);
  await provider.stopInstance(id);
  await provider.wakeInstance(id);
  const resume = defined(fake.requests.find((r) => r.path === `/sandboxes/${id}/resume`));
  assertEquals(resume.body, { ttlSeconds: 3600 });
});

Deno.test("keepAlive pushes the auto-stop deadline out with PATCH ttlSeconds", async () => {
  const fake = new FakeBoat();
  const provider = new BoatProvider({ ...CONFIG, autoStopMinutes: 90 }, { fetch: fake.fetch });
  const { providerResourceId: id } = await provider.createInstance(PARAMS);
  await provider.keepAlive(id);
  const req = defined(fake.requests.at(-1));
  assertEquals(req.method, "PATCH");
  assertEquals(req.path, `/sandboxes/${id}`);
  assertEquals(req.body, { ttlSeconds: 5400 });
});

Deno.test("getInstance reports the provider's updatedAt as stateChangedAt", async () => {
  const { fake, provider } = setup();
  fake.sandboxes.set("bx_t", {
    id: "bx_t", name: "t", state: "archived", type: "small", labels: {}, ip: null, sshEndpoint: null,
    updatedAt: "2026-10-28T10:01:00Z",
  } as unknown as Parameters<typeof fake.sandboxes.set>[1]);
  assertEquals((await provider.getInstance("bx_t")).stateChangedAt, "2026-10-28T10:01:00Z");
});
