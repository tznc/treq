// Turns a provider kind into its adapter. Every Edge Function gets its
// provider here, so adding a provider or a stub is a change in one place
// (docs/design/sandbox-provider-adapter.md, D1).
//
// `credential` is a BYOS user's own key (byos.ts loads it server-side).
// Each adapter's config function takes it in place of treq's env secret,
// so a new provider kind plugs in by accepting it the same way.

import { BOAT_CAPABILITIES, BoatProvider, boatConfigFromEnv } from "./boat-adapter.ts";
import { DAYTONA_CAPABILITIES, DaytonaProvider, daytonaConfigFromEnv } from "./daytona-adapter.ts";
import { E2B_CAPABILITIES, E2BProvider, e2bConfigFromEnv } from "./e2b-adapter.ts";
import { FLY_CAPABILITIES, FlyMachinesProvider, flyConfigFromEnv } from "./fly-adapter.ts";
import { isProviderStubEnabled, LocalProvider } from "./local-provider.ts";
import {
  type ProviderCapabilities,
  type ProviderCredential,
  ProviderError,
  type ProviderKind,
  type SandboxProvider,
} from "./provider.ts";

/**
 * How callers reach a machine's provider. Pass the placement id whenever
 * there is one: a BYOS placement must use its owner's key, never treq's
 * (byos.ts `providerForPlacement`).
 */
export type ProviderResolver = (kind: ProviderKind, placementId?: string | null) => SandboxProvider;

const VENDOR_CAPABILITIES: Partial<Record<ProviderKind, Readonly<ProviderCapabilities>>> = {
  boat: BOAT_CAPABILITIES,
  daytona: DAYTONA_CAPABILITIES,
  e2b: E2B_CAPABILITIES,
  fly_machines: FLY_CAPABILITIES,
};

export function providerFor(
  kind: ProviderKind,
  env: (key: string) => string | undefined = (key) => Deno.env.get(key),
  credential?: ProviderCredential,
): SandboxProvider {
  // No BYOS for Fly yet: never run a user's workspace on treq's account.
  if (kind === "fly_machines" && credential) {
    throw new ProviderError("invalid_request", "fly_machines does not take a BYOS credential yet");
  }
  // Under the stub, local service-qa never calls a real vendor. The
  // stand-in keeps the provider's kind and capabilities.
  const stubCapabilities = Object.hasOwn(VENDOR_CAPABILITIES, kind) ? VENDOR_CAPABILITIES[kind] : undefined;
  if (stubCapabilities && isProviderStubEnabled(env)) return new LocalProvider(kind, stubCapabilities);
  switch (kind) {
    case "boat":
      return new BoatProvider(boatConfigFromEnv(env, credential));
    case "daytona":
      return new DaytonaProvider(daytonaConfigFromEnv(env, credential));
    case "e2b":
      return new E2BProvider(e2bConfigFromEnv(env, credential));
    case "fly_machines":
      return new FlyMachinesProvider(flyConfigFromEnv(env));
    case "local":
      return new LocalProvider();
    default:
      throw new ProviderError("invalid_request", `unknown provider kind: ${kind as string}`);
  }
}
