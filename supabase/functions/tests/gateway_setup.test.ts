// Bootstrapping treq-gateway, and re-running
// pidfile-supervised services after a wake. No network.

import { assert, assertEquals, assertMatch, assertRejects } from "https://deno.land/std@0.224.0/assert/mod.ts";
import {
  type GatewaySecretStore,
  provisionGateway,
  reensureServicesAfterWake,
} from "../_shared/remote/machine-setup.ts";
import { type ProviderCapabilities, ProviderError, type ServiceSpec } from "../_shared/remote/provider.ts";
import { BOAT_CAPABILITIES } from "../_shared/remote/boat-adapter.ts";
import {
  gatewayServiceSpec,
  installGatewaySecretCommand,
  MANAGED_SSHD_LAUNCHER,
} from "../_shared/remote/ssh-vm-config.ts";
import { defined, fakeProvider } from "./helpers/remote_fakes.ts";

// `launcherInstalled` answers the `test -x` for sshd's launcher.
function recorder(caps: Partial<ProviderCapabilities>, launcherInstalled = () => true) {
  const execs: string[] = [];
  const envs: Array<Record<string, string>> = [];
  const services: Array<[string, ServiceSpec]> = [];
  const provider = fakeProvider({
    kind: "boat",
    capabilities: () => ({ ...BOAT_CAPABILITIES, ...caps }),
    execOnMachine: (_id, cmd, _t, env = {}) => {
      execs.push(cmd.join(" "));
      envs.push(env);
      const missing = cmd.join(" ") === `test -x ${MANAGED_SSHD_LAUNCHER}` && !launcherInstalled();
      return Promise.resolve({ exitCode: missing ? 1 : 0, stdout: "", stderr: "" });
    },
    ensureService: (_id, name, spec) => {
      services.push([name, spec]);
      return Promise.resolve();
    },
  });
  return { provider, execs, envs, services };
}

function memoryStore(initial: Record<string, string> = {}): GatewaySecretStore & { saved: Record<string, string> } {
  const saved = { ...initial };
  return {
    saved,
    load: (id) => Promise.resolve(saved[id] ?? null),
    save: (id, secret) => {
      saved[id] = secret;
      return Promise.resolve();
    },
  };
}

Deno.test("provisionGateway saves a new secret, writes it and runs `treq gateway`, with no download", async () => {
  const { provider, execs, envs, services } = recorder({});
  const store = memoryStore();
  await provisionGateway(store, provider, "m1", "pl-1");
  const secret = defined(store.saved["pl-1"]);
  assertMatch(secret, /^[0-9a-f]{64}$/);
  assertEquals(execs.length, 1);
  const exec = defined(execs[0]);
  assert(exec.includes("/etc/treq/gateway.secret"));
  // The secret travels in the exec environment, never the command line.
  assert(!exec.includes(secret), "secret must not be in the command line");
  assertEquals(envs[0]?.TREQ_GATEWAY_SECRET, secret);
  assert(exec.includes('"$TREQ_GATEWAY_SECRET"'));
  assert(!execs.some((e) => e.includes("curl") || e.includes("sha256sum")));
  assertEquals(services, [["treq-gateway", {
    cmd: "/usr/local/bin/treq",
    args: ["gateway", "--placement-id", "pl-1", "--secret-file", "/etc/treq/gateway.secret", "--listen", "127.0.0.1:8022"],
  }]]);
});

Deno.test("provisionGateway takes the provider's listen address", async () => {
  const { provider, services } = recorder({});
  await provisionGateway(memoryStore(), provider, "m1", "pl-1", "0.0.0.0:8022");
  assert(defined(services[0])[1].args.join(" ").endsWith("--listen 0.0.0.0:8022"));
  assertEquals(gatewayServiceSpec("pl-1", "0.0.0.0:8022").args.at(-1), "0.0.0.0:8022");
});

Deno.test("provisionGateway rotates the secret, saving it before writing it", async () => {
  const existing = "c".repeat(64);
  const store = memoryStore({ "pl-1": existing });
  let savedBeforeExec: string | undefined;
  const { provider, envs } = recorder({});
  const exec = provider.execOnMachine.bind(provider);
  (provider as { execOnMachine: unknown }).execOnMachine = (...a: Parameters<typeof exec>) => {
    savedBeforeExec = store.saved["pl-1"];
    return exec(...a);
  };
  await provisionGateway(store, provider, "m1", "pl-1");
  assertMatch(defined(store.saved["pl-1"]), /^[0-9a-f]{64}$/);
  assert(store.saved["pl-1"] !== existing, "a new secret on each provision");
  assertEquals(savedBeforeExec, store.saved["pl-1"]);
  assertEquals(envs[0]?.TREQ_GATEWAY_SECRET, store.saved["pl-1"]);
});

Deno.test("the secret file is written mode 600", () => {
  const secretScript = installGatewaySecretCommand().join(" ");
  assert(secretScript.includes("chmod 600"));
  assert(!/[0-9a-f]{64}/.test(secretScript));
});

Deno.test("reensureServicesAfterWake re-runs sshd and the gateway under a pidfile supervisor", async () => {
  const { provider, services } = recorder({ serviceSupervisor: false });
  await reensureServicesAfterWake(provider, "m1", "pl-1");
  assertEquals(services.map(([n]) => n), ["treq-sshd", "treq-gateway"]);
});

Deno.test("a wake finishes the setup instead of starting services when sshd was never installed", async () => {
  let installed = false;
  const { provider, services } = recorder({ serviceSupervisor: false }, () => installed);
  let finished = 0;
  await reensureServicesAfterWake(provider, "m1", "pl-1", () => {
    finished++;
    installed = true;
    return Promise.resolve();
  });
  assertEquals(finished, 1);
  assertEquals(services, []);
});

Deno.test("a wake fails when finishing the setup still leaves sshd missing", async () => {
  const { provider } = recorder({ serviceSupervisor: false }, () => false);
  const err = await assertRejects(
    () => reensureServicesAfterWake(provider, "m1", "pl-1", () => Promise.resolve()),
    ProviderError,
  );
  assertMatch(err.message, /sshd is not installed/);
});

Deno.test("a wake with sshd installed restarts the services and skips the setup", async () => {
  const { provider, services } = recorder({ serviceSupervisor: false });
  await reensureServicesAfterWake(provider, "m1", "pl-1", () => Promise.reject(new Error("setup ran")));
  assertEquals(services.map(([n]) => n), ["treq-sshd", "treq-gateway"]);
});

Deno.test("reensureServicesAfterWake leaves a real service supervisor alone", async () => {
  const { provider, services } = recorder({ serviceSupervisor: true });
  await reensureServicesAfterWake(provider, "m1", "pl-1");
  assertEquals(services.length, 0);
});

Deno.test("the gateway listens where the provider says, loopback by default", async () => {
  const { provider, services } = recorder({ serviceSupervisor: false });
  await reensureServicesAfterWake(provider, "m1", "pl-1");
  assert(defined(services[1])[1].args.join(" ").includes("--listen 127.0.0.1:8022"));
  (provider as { gatewayListenAddress?: string }).gatewayListenAddress = "0.0.0.0:8022";
  await reensureServicesAfterWake(provider, "m1", "pl-1");
  assert(defined(services[3])[1].args.join(" ").includes("--listen 0.0.0.0:8022"));
});
