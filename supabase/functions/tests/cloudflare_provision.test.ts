// Deploying the sandbox bridge Worker into a BYOS user's Cloudflare account
// with their API token, checked against an in-memory fake of Cloudflare's
// REST API. No network.
//
// Run with: `deno test --allow-env supabase/functions/tests/cloudflare_provision.test.ts`

import { defined } from "./remote_fakes.ts";
import { assert, assertEquals, assertMatch, assertRejects } from "https://deno.land/std@0.224.0/assert/mod.ts";
import {
  BRIDGE_IMAGE,
  CloudflareProvider,
  cloudflareConfigFromEnv,
  parseCloudflareCredential,
  provisionBridge,
} from "../_shared/remote/cloudflare-adapter.ts";
import { ProviderError } from "../_shared/remote/provider.ts";
import { FakeCloudflareApi } from "./fake_cloudflare_api.ts";

const API = "https://api.cloudflare.example.invalid/client/v4";

function setup() {
  const fake = new FakeCloudflareApi();
  const deps = { fetch: fake.fetch, apiBase: API, randomToken: () => "bridge-secret" };
  return { fake, deps };
}

Deno.test("a pasted key is <account id>:<api token>; a stored key adds |<bridge url>#<bridge token>", () => {
  assertEquals(parseCloudflareCredential("acct1:cf_api"), { accountId: "acct1", apiToken: "cf_api" });
  assertEquals(parseCloudflareCredential("acct1:cf_api|https://treq-sandbox.acme.workers.dev#t"), {
    accountId: "acct1",
    apiToken: "cf_api",
    bridgeUrl: "https://treq-sandbox.acme.workers.dev",
    token: "t",
  });
  assertEquals(parseCloudflareCredential("https://b.dev#t"), { bridgeUrl: "https://b.dev", token: "t" });
});

Deno.test("provisioning creates KV, uploads the bridge, enables workers.dev and links a container app per size", async () => {
  const { fake, deps } = setup();
  const stored = await provisionBridge({ accountId: "acct1", apiToken: "cf_api" }, deps);
  assertEquals(stored, "acct1:cf_api|https://treq-sandbox.acme.workers.dev#bridge-secret");

  assertEquals([...fake.kv.values()], ["treq-sandbox-types"]);
  const script = defined(fake.script);
  assert(script.module.length > 1000, "uploads the bundled bridge");
  const bindings = script.metadata.bindings as Array<Record<string, string>>;
  assert(bindings.some((b) => b.type === "secret_text" && b.name === "BRIDGE_TOKEN" && b.text === "bridge-secret"));
  assert(bindings.some((b) => b.type === "kv_namespace" && b.name === "TYPES" && b.namespace_id === "kv1"));
  for (const cls of ["SandboxS", "SandboxM"]) {
    assert(bindings.some((b) => b.type === "durable_object_namespace" && b.class_name === cls));
  }
  assertEquals(script.metadata.migrations, { new_tag: "v1", new_sqlite_classes: ["SandboxS", "SandboxM"] });
  assert(fake.workersDev);

  const apps = fake.applications.map((a) => [
    a.name,
    (a.configuration as { image: string; instance_type: string }).instance_type,
    (a.configuration as { image: string }).image,
    (a.durable_objects as { namespace_id: string }).namespace_id,
  ]);
  assertEquals(apps, [
    ["treq-sandbox-s", "standard-3", BRIDGE_IMAGE, "ns-SandboxS"],
    ["treq-sandbox-m", "standard-4", BRIDGE_IMAGE, "ns-SandboxM"],
  ]);
  assertMatch(BRIDGE_IMAGE, /^docker\.io\/cloudflare\/sandbox:\d+\.\d+\.\d+$/);
});

Deno.test("provisioning again reuses KV and container apps and skips the DO migration", async () => {
  const { fake, deps } = setup();
  await provisionBridge({ accountId: "acct1", apiToken: "cf_api" }, deps);
  await provisionBridge({ accountId: "acct1", apiToken: "cf_api" }, deps);
  assertEquals(fake.kv.size, 1);
  assertEquals(fake.applications.length, 2);
  assertEquals(defined(fake.script).metadata.migrations, undefined);
});

Deno.test("a rejected token is unauthorized", async () => {
  const { deps } = setup();
  const err = await assertRejects(() => provisionBridge({ accountId: "acct1", apiToken: "nope" }, deps), ProviderError);
  assertEquals(err.kind, "unauthorized");
});

Deno.test("the adapter provisions from a pasted key and needs a bridge for everything else", async () => {
  const { fake, deps } = setup();
  const pasted = cloudflareConfigFromEnv(() => undefined, { apiKey: "acct1:cf_api" });
  const provider = new CloudflareProvider(pasted, deps);
  await provider.validateCredentials();
  assertEquals(await provider.provisionCredential(), "acct1:cf_api|https://treq-sandbox.acme.workers.dev#bridge-secret");
  assert(fake.script);
  const err = await assertRejects(() => provider.getInstance("x"), ProviderError);
  assertEquals(err.kind, "invalid_request");
});
