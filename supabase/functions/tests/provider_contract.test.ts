// One contract suite run against every SandboxProvider, so a new adapter
// proves the same lifecycle semantics the control plane relies on
// (docs/design/sandbox-provider-adapter.md, §3). No network: Boat runs
// against an in-memory fake of its API.
//
// Run with: `deno test --allow-env supabase/functions/tests/provider_contract.test.ts`

import {
  assert,
  assertEquals,
  assertRejects,
} from "https://deno.land/std@0.224.0/assert/mod.ts";
import { BoatProvider } from "../_shared/remote/boat-adapter.ts";
import { DaytonaProvider } from "../_shared/remote/daytona-adapter.ts";
import { FlyMachinesProvider } from "../_shared/remote/fly-adapter.ts";
import { LocalProvider } from "../_shared/remote/local-provider.ts";
import {
  DEFAULT_PROVIDER_AUTOSTOP_MIN,
  type CreateInstanceParams,
  type ManagedInstanceState,
  ProviderError,
  type SandboxProvider,
} from "../_shared/remote/provider.ts";
import { DEFAULT_IDLE_TIMEOUT_MIN } from "../_shared/remote/idle-controller.ts";
import { FakeBoat } from "./fake_boat.ts";
import { FakeDaytona } from "./fake_daytona.ts";
import { FakeFly } from "./fake_fly.ts";
import { E2BProvider } from "../_shared/remote/e2b-adapter.ts";
import { FakeE2B } from "./fake_e2b.ts";

const E2B_CONFIG = {
  baseUrl: "https://api.e2b.example.invalid",
  apiKey: "e2b_test",
  template: "treq",
  autoStopMinutes: 60,
};

const FLY_CONFIG = {
  baseUrl: "https://fly.example.invalid",
  apiToken: "fly_test",
  app: "treq-test",
  image: "img",
};

const STATES: ReadonlySet<ManagedInstanceState> = new Set([
  "unprovisioned",
  "provisioning",
  "bootstrapping",
  "installing_access",
  "verifying",
  "ready",
  "suspended",
  "waking",
  "reprovisioning",
  "degraded",
  "failed",
  "deleting",
  "deleted",
]);

interface Contender {
  name: string;
  make(): SandboxProvider;
  // The provider's own machine type; adapters reject types they do not know.
  machineType?: string;
  // Has no real machine, so no TCP path to sshd.
  standIn?: boolean;
}

const contenders: Contender[] = [
  { name: "Local", standIn: true, make: () => new LocalProvider() },
  {
    name: "Boat",
    make: () =>
      new BoatProvider(
        { baseUrl: "https://boat.example.invalid/api/v1", apiToken: "boat_test" },
        { fetch: new FakeBoat().fetch },
      ),
  },
  {
    name: "Daytona",
    machineType: "s",
    make: () =>
      new DaytonaProvider(
        { baseUrl: "https://daytona.example.invalid/api", apiKey: "dtn_test" },
        { fetch: new FakeDaytona().fetch },
      ),
  },
  {
    name: "E2B",
    machineType: "s",
    make: () => new E2BProvider(E2B_CONFIG, { fetch: new FakeE2B().fetch }),
  },
  {
    name: "Fly Machines",
    machineType: "shared-cpu-2x:4096",
    make: () => new FlyMachinesProvider(FLY_CONFIG, { fetch: new FakeFly().fetch }),
  },
];

function params(machineType = "small"): CreateInstanceParams {
  const owner = crypto.randomUUID();
  return {
    ownerUserId: owner,
    region: "us_east",
    sizePreset: "small",
    manifestVersion: 1,
    idempotencyKey: `create-${owner}`,
    machineType,
  };
}

for (const c of contenders) {
  Deno.test(`${c.name}: create with the same idempotency key returns the same resource`, async () => {
    const provider = c.make();
    const p = params(c.machineType);
    const first = await provider.createInstance(p);
    const second = await provider.createInstance(p);
    assertEquals(second.providerResourceId, first.providerResourceId);
    assert(STATES.has(first.state));
  });

  Deno.test(`${c.name}: stop then wake moves between suspended and ready`, async () => {
    const provider = c.make();
    const { providerResourceId: id } = await provider.createInstance(params(c.machineType));
    await provider.stopInstance(id);
    assertEquals((await provider.getInstance(id)).state, "suspended");
    await provider.wakeInstance(id);
    assertEquals((await provider.getInstance(id)).state, "ready");
  });

  Deno.test(`${c.name}: deleting a missing machine succeeds`, async () => {
    const provider = c.make();
    const { providerResourceId: id } = await provider.createInstance(params(c.machineType));
    await provider.deleteInstance(id);
    await provider.deleteInstance(id);
    const err = await assertRejects(async () => await provider.getInstance(id), ProviderError);
    assertEquals(err.kind, "not_found");
  });

  Deno.test(`${c.name}: exec and ensureService on a missing machine are not_found`, async () => {
    const provider = c.make();
    const exec = await assertRejects(
      async () => await provider.execOnMachine("missing-machine", ["true"]),
      ProviderError,
    );
    assertEquals(exec.kind, "not_found");
    const svc = await assertRejects(
      async () => await provider.ensureService("missing-machine", "sshd", { cmd: "/usr/sbin/sshd", args: ["-D"] }),
      ProviderError,
    );
    assertEquals(svc.kind, "not_found");
  });

  Deno.test(`${c.name}: exec on a live machine returns a result`, async () => {
    const provider = c.make();
    const { providerResourceId: id } = await provider.createInstance(params(c.machineType));
    const result = await provider.execOnMachine(id, ["true"]);
    assertEquals(result.exitCode, 0);
    await provider.ensureService(id, "sshd", { cmd: "/usr/sbin/sshd", args: ["-D"] });
  });

  Deno.test(`${c.name}: capabilities have the documented shape`, () => {
    const caps = c.make().capabilities();
    assertEquals(Object.keys(caps).sort(), [
      "billingGranularitySec",
      "execApi",
      "serviceSupervisor",
      "stopKeepsDisk",
      "tcpProxy",
    ]);
    for (const key of ["execApi", "serviceSupervisor", "stopKeepsDisk", "tcpProxy"] as const) {
      assertEquals(typeof caps[key], "boolean");
    }
    assert(Number.isInteger(caps.billingGranularitySec) && caps.billingGranularitySec > 0);
  });

  // D4: a provider with a TCP path names a WebSocket endpoint speaking the
  // gateway proxy protocol (gateway-proxy.ts); a stand-in refuses with unavailable.
  Deno.test(`${c.name}: gatewayEndpoint names a ws(s) URL, or is unavailable`, async () => {
    const provider = c.make();
    const { providerResourceId: id } = await provider.createInstance(params(c.machineType));
    if (c.standIn) {
      const err = await assertRejects(() => provider.gatewayEndpoint(id), ProviderError);
      assertEquals(err.kind, "unavailable");
      return;
    }
    const endpoint = await provider.gatewayEndpoint(id);
    assert(/^wss?:/.test(endpoint.url), endpoint.url);
    assertEquals(typeof endpoint.headers, "object");
  });
}

// HTTP adapters only: vendor status codes map to ProviderError kinds. The
// shared rows hold for every vendor; `extra` pins each vendor's exceptions.
type Kind = ProviderError["kind"];
const SHARED_STATUSES: Array<[number, Kind]> = [
  [404, "not_found"],
  [409, "already_exists"],
  [429, "quota_exceeded"],
  [400, "invalid_request"],
  [500, "unavailable"],
  [503, "unavailable"],
];

const httpContenders: Array<{ name: string; make(forcedStatus: number): SandboxProvider; extra: Array<[number, Kind]> }> = [
  {
    name: "Boat",
    make: (status) =>
      new BoatProvider(
        { baseUrl: "https://boat.example.invalid/api/v1", apiToken: "boat_test" },
        { fetch: Object.assign(new FakeBoat(), { forcedStatus: status }).fetch },
      ),
    extra: [[401, "unauthorized"], [403, "unauthorized"], [402, "quota_exceeded"], [408, "other"]],
  },
  {
    name: "Daytona",
    make: (status) =>
      new DaytonaProvider(
        { baseUrl: "https://daytona.example.invalid/api", apiKey: "dtn_test" },
        { fetch: Object.assign(new FakeDaytona(), { forcedStatus: status }).fetch },
      ),
    extra: [[401, "unauthorized"], [403, "unauthorized"], [408, "timeout"]],
  },
  {
    name: "E2B",
    make: (status) => new E2BProvider(E2B_CONFIG, { fetch: Object.assign(new FakeE2B(), { forcedStatus: status }).fetch }),
    extra: [[401, "unauthorized"], [403, "unauthorized"], [504, "timeout"]],
  },
  {
    name: "Fly Machines",
    make: (status) =>
      new FlyMachinesProvider(FLY_CONFIG, { fetch: Object.assign(new FakeFly(), { forcedStatus: status }).fetch }),
    extra: [[401, "other"], [403, "quota_exceeded"], [412, "invalid_request"]],
  },
];

for (const c of httpContenders) {
  for (const [status, kind] of [...SHARED_STATUSES, ...c.extra]) {
    Deno.test(`${c.name}: HTTP ${status} maps to ${kind}`, async () => {
      const err = await assertRejects(() => c.make(status).getInstance("id_1"), ProviderError);
      assertEquals(err.kind, kind);
    });
  }
}

// Auto-stop backstop: a provider timer, when there is one, must outlast the
// idle controller so it only fires when the controller has failed, and
// keepAlive must push it out. Fly has none: Fly Proxy's autostop would cut
// open WebSockets, so the idle controller alone stops Fly machines.
const backstops: { name: string; create(): Promise<{ provider: SandboxProvider; seconds: number | null }> }[] = [
  {
    name: "Boat",
    async create() {
      const fake = new FakeBoat();
      const provider = new BoatProvider({ baseUrl: "https://boat.example.invalid/api/v1", apiToken: "t" }, { fetch: fake.fetch });
      await provider.createInstance(params());
      const body = fake.requests.find((r) => r.method === "POST" && r.path === "/sandboxes")!.body as { ttlSeconds: number };
      return { provider, seconds: body.ttlSeconds };
    },
  },
  {
    name: "Daytona",
    async create() {
      const fake = new FakeDaytona();
      const provider = new DaytonaProvider({ baseUrl: "https://daytona.example.invalid/api", apiKey: "k" }, { fetch: fake.fetch });
      await provider.createInstance(params("s"));
      const body = fake.requests.find((r) => r.method === "POST" && r.path === "/api/sandbox")!.body as { autoStopInterval: number };
      return { provider, seconds: body.autoStopInterval * 60 };
    },
  },
  {
    name: "E2B",
    async create() {
      const fake = new FakeE2B();
      const provider = new E2BProvider({ ...E2B_CONFIG, autoStopMinutes: undefined }, { fetch: fake.fetch });
      await provider.createInstance(params("s"));
      const body = fake.requests.find((r) => r.method === "POST" && r.path.endsWith("/sandboxes"))!.body as { timeout: number };
      return { provider, seconds: body.timeout };
    },
  },
  {
    name: "Fly Machines",
    async create() {
      const fake = new FakeFly();
      const provider = new FlyMachinesProvider(FLY_CONFIG, { fetch: fake.fetch });
      const { providerResourceId: id } = await provider.createInstance(params("shared-cpu-2x:4096"));
      const services = fake.machines.get(id)!.config.services as { autostop: string }[];
      assert(services.every((s) => s.autostop === "off"), "Fly autostop is off");
      return { provider, seconds: null };
    },
  },
];

for (const b of backstops) {
  Deno.test(`${b.name}: the auto-stop backstop outlasts the idle controller`, async () => {
    const { provider, seconds } = await b.create();
    if (seconds === null) return;
    assertEquals(seconds, DEFAULT_PROVIDER_AUTOSTOP_MIN * 60);
    assert(seconds > DEFAULT_IDLE_TIMEOUT_MIN * 60);
    assertEquals(typeof provider.keepAlive, "function", "keepAlive pushes the timer out");
  });
}
