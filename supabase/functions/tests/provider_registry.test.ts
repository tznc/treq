// Unit tests for the sandbox provider registry: the one place an Edge
// Function turns a provider kind into an adapter. No network.
//
// Run with: `deno test --allow-env supabase/functions/tests/provider_registry.test.ts`

import {
  assertEquals,
  assertInstanceOf,
  assertThrows,
} from "https://deno.land/std@0.224.0/assert/mod.ts";
import { providerFor } from "../_shared/remote/provider-registry.ts";
import { ProviderError } from "../_shared/remote/provider.ts";
import { BOAT_CAPABILITIES, BoatProvider } from "../_shared/remote/boat-adapter.ts";
import { DAYTONA_CAPABILITIES, DaytonaProvider } from "../_shared/remote/daytona-adapter.ts";
import { E2B_CAPABILITIES, E2BProvider } from "../_shared/remote/e2b-adapter.ts";
import { FLY_CAPABILITIES, FlyMachinesProvider } from "../_shared/remote/fly-adapter.ts";
import { LocalProvider } from "../_shared/remote/local-provider.ts";

function envOf(values: Record<string, string>): (key: string) => string | undefined {
  return (key) => values[key];
}

Deno.test("an unknown provider kind is rejected", () => {
  const err = assertThrows(
    () => providerFor("nope" as never, envOf({ TREQ_REMOTE_PROVIDER_STUB: "1" })),
    ProviderError,
  );
  assertEquals(err.kind, "invalid_request");
});

Deno.test("boat resolves to the real adapter when the stub is off", () => {
  const provider = providerFor(
    "boat",
    envOf({ TREQ_BOAT_API_URL: "https://example.invalid", TREQ_BOAT_API_TOKEN: "t" }),
  );
  assertInstanceOf(provider, BoatProvider);
  assertEquals(provider.kind, "boat");
});

Deno.test("the real boat adapter needs its secrets", () => {
  const err = assertThrows(() => providerFor("boat", envOf({})), ProviderError);
  assertEquals(err.kind, "invalid_request");
});

Deno.test("boat resolves to an in-memory provider under TREQ_REMOTE_PROVIDER_STUB", () => {
  const provider = providerFor("boat", envOf({ TREQ_REMOTE_PROVIDER_STUB: "1" }));
  assertInstanceOf(provider, LocalProvider);
  // Stub mode keeps the boat kind and capabilities, so callers take Boat's paths.
  assertEquals(provider.kind, "boat");
  assertEquals(provider.capabilities(), BOAT_CAPABILITIES);
});

Deno.test("fly_sprites is no longer a provider kind", () => {
  const err = assertThrows(
    () => providerFor("fly_sprites" as never, envOf({ TREQ_REMOTE_PROVIDER_STUB: "1" })),
    ProviderError,
  );
  assertEquals(err.kind, "invalid_request");
});

Deno.test("the old TREQ_REMOTE_SPRITES_STUB name no longer turns the stub on", () => {
  const err = assertThrows(
    () => providerFor("boat", envOf({ ["TREQ_REMOTE_" + "SPRITES_STUB"]: "1" })),
    ProviderError,
  );
  assertEquals(err.kind, "invalid_request");
});

Deno.test("local always resolves to LocalProvider", () => {
  for (const env of [envOf({}), envOf({ TREQ_REMOTE_PROVIDER_STUB: "1" })]) {
    const provider = providerFor("local", env);
    assertInstanceOf(provider, LocalProvider);
    assertEquals(provider.kind, "local");
  }
});

Deno.test("daytona resolves to the real adapter when the stub is off", () => {
  const provider = providerFor("daytona", envOf({ TREQ_DAYTONA_API_KEY: "k" }));
  assertInstanceOf(provider, DaytonaProvider);
  assertEquals(provider.kind, "daytona");
});

Deno.test("the real daytona adapter needs its API key", () => {
  const err = assertThrows(() => providerFor("daytona", envOf({})), ProviderError);
  assertEquals(err.kind, "invalid_request");
});

Deno.test("daytona resolves to an in-memory provider under TREQ_REMOTE_PROVIDER_STUB", () => {
  const provider = providerFor("daytona", envOf({ TREQ_REMOTE_PROVIDER_STUB: "1" }));
  assertInstanceOf(provider, LocalProvider);
  assertEquals(provider.kind, "daytona");
  assertEquals(provider.capabilities(), DAYTONA_CAPABILITIES);
});
Deno.test("e2b resolves to the real adapter when the stub is off", () => {
  const provider = providerFor("e2b", envOf({ TREQ_E2B_API_KEY: "k" }));
  assertInstanceOf(provider, E2BProvider);
  assertEquals(provider.kind, "e2b");
});

Deno.test("the real e2b adapter needs its API key", () => {
  const err = assertThrows(() => providerFor("e2b", envOf({})), ProviderError);
  assertEquals(err.kind, "invalid_request");
});

Deno.test("e2b takes a BYOS credential in place of treq's key", () => {
  const provider = providerFor("e2b", envOf({}), { apiKey: "user-key" });
  assertInstanceOf(provider, E2BProvider);
  assertEquals(typeof provider.validateCredentials, "function");
});

Deno.test("e2b resolves to an in-memory provider under TREQ_REMOTE_PROVIDER_STUB", () => {
  const provider = providerFor("e2b", envOf({ TREQ_REMOTE_PROVIDER_STUB: "1" }));
  assertInstanceOf(provider, LocalProvider);
  assertEquals(provider.kind, "e2b");
  assertEquals(provider.capabilities(), E2B_CAPABILITIES);
});

Deno.test("fly_machines resolves to the real adapter when the stub is off", () => {
  const provider = providerFor(
    "fly_machines",
    envOf({ TREQ_FLY_API_TOKEN: "t", TREQ_FLY_APP: "a", TREQ_FLY_IMAGE: "i" }),
  );
  assertInstanceOf(provider, FlyMachinesProvider);
  assertEquals(provider.kind, "fly_machines");
});

Deno.test("fly_machines resolves to an in-memory provider under TREQ_REMOTE_PROVIDER_STUB", () => {
  const provider = providerFor("fly_machines", envOf({ TREQ_REMOTE_PROVIDER_STUB: "1" }));
  assertInstanceOf(provider, LocalProvider);
  assertEquals(provider.kind, "fly_machines");
  assertEquals(provider.capabilities(), { ...FLY_CAPABILITIES });
});

Deno.test("fly_machines refuses a BYOS credential instead of falling back to treq's account", () => {
  const err = assertThrows(
    () =>
      providerFor(
        "fly_machines",
        envOf({ TREQ_FLY_API_TOKEN: "managed", TREQ_FLY_APP: "a", TREQ_FLY_IMAGE: "i" }),
        { apiKey: "user-key" },
      ),
    ProviderError,
  );
  assertEquals(err.kind, "invalid_request");
});
