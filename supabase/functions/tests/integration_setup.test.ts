// Installing a placement's integration credential: the secret its sandbox
// signs integration-proxy requests with. No network.

import { assert, assertEquals, assertMatch } from "https://deno.land/std@0.224.0/assert/mod.ts";
import {
  installIntegrationConfigCommand,
  INTEGRATION_CONFIG_ENV,
  INTEGRATION_CONFIG_FILE,
  type IntegrationSecretStore,
  provisionIntegrationCredential,
} from "../_shared/remote/integration-credential.ts";
import type { SandboxProvider } from "../_shared/remote/provider.ts";

function recorder() {
  const execs: string[] = [];
  const envs: Array<Record<string, string>> = [];
  const provider = {
    execOnMachine: (_id: string, cmd: string[], _t?: number, env: Record<string, string> = {}) => {
      execs.push(cmd.join(" "));
      envs.push(env);
      return Promise.resolve({ exitCode: 0, stdout: "", stderr: "" });
    },
  } as unknown as SandboxProvider;
  return { provider, execs, envs };
}

function memoryStore(): IntegrationSecretStore & { saved: Record<string, string> } {
  const saved: Record<string, string> = {};
  return {
    saved,
    save: (id, secret) => {
      saved[id] = secret;
      return Promise.resolve();
    },
  };
}

const PROXY = "https://proj.supabase.co/functions/v1/integration-proxy";

Deno.test("provisionIntegrationCredential saves a new secret, then writes the config", async () => {
  const { provider, execs, envs } = recorder();
  const store = memoryStore();
  await provisionIntegrationCredential(store, provider, "m1", "pl-1", PROXY);
  const secret = store.saved["pl-1"];
  assertMatch(secret, /^[0-9a-f]{64}$/);
  assertEquals(execs.length, 1);
  assert(execs[0].includes(INTEGRATION_CONFIG_FILE));
  // The secret travels in the exec environment, never the command line.
  assert(!execs[0].includes(secret), "secret must not be in the command line");
  assertEquals(JSON.parse(envs[0][INTEGRATION_CONFIG_ENV]), {
    placement_id: "pl-1",
    proxy_url: PROXY,
    secret,
  });
});

Deno.test("provisionIntegrationCredential rotates the secret on every provision", async () => {
  const { provider } = recorder();
  const store = memoryStore();
  await provisionIntegrationCredential(store, provider, "m1", "pl-1", PROXY);
  const first = store.saved["pl-1"];
  await provisionIntegrationCredential(store, provider, "m1", "pl-1", PROXY);
  assert(store.saved["pl-1"] !== first);
});

Deno.test("provisionIntegrationCredential saves before it writes, so a failed save writes nothing", async () => {
  const { provider, execs } = recorder();
  const failing: IntegrationSecretStore = { save: () => Promise.reject(new Error("vault down")) };
  let threw = false;
  try {
    await provisionIntegrationCredential(failing, provider, "m1", "pl-1", PROXY);
  } catch {
    threw = true;
  }
  assert(threw);
  assertEquals(execs.length, 0);
});

Deno.test("the config is readable by the sandbox's treq user, not by everyone", () => {
  const script = installIntegrationConfigCommand().join(" ");
  assert(script.includes("/etc/treq/integration.json"));
  assert(script.includes("chmod 640"));
  assert(script.includes("chgrp treq"));
  assert(!/[0-9a-f]{64}/.test(script));
});
