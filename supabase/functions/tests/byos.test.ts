// Unit tests for bring your own sandbox (BYOS; _shared/remote/byos.ts and
// the adapters' credential handling). Fake fetch, fake stores; no network.
//
// Run with: `deno test --allow-env supabase/functions/tests/byos.test.ts`

import { defined } from "./remote_fakes.ts";
import {
  assert,
  assertEquals,
  assertInstanceOf,
  assertRejects,
} from "https://deno.land/std@0.224.0/assert/mod.ts";
import { BoatProvider, boatConfigFromEnv } from "../_shared/remote/boat-adapter.ts";
import { DaytonaProvider, daytonaConfigFromEnv } from "../_shared/remote/daytona-adapter.ts";
import { LocalProvider } from "../_shared/remote/local-provider.ts";
import { ProviderError, type ProviderKind, type SandboxProvider } from "../_shared/remote/provider.ts";
import { providerFor } from "../_shared/remote/provider-registry.ts";
import {
  type ByosCredentialStore,
  handleByosAction,
  isOperatorMovable,
  parsePlacementChoice,
  planByosPlacement,
  providerErrorCode,
  providerForPlacement,
} from "../_shared/remote/byos.ts";

function envOf(values: Record<string, string>): (key: string) => string | undefined {
  return (key) => values[key];
}

function fetchAnswering(status: number, body = "{}", seen: Request[] = []): typeof fetch {
  return (input: RequestInfo | URL, init?: RequestInit) => {
    seen.push(new Request(input, init));
    return Promise.resolve(new Response(body, { status, headers: { "Content-Type": "application/json" } }));
  };
}

// ── Adapters: unauthorized and validateCredentials ─────────────────────

for (const status of [401, 403]) {
  Deno.test(`Boat ${status} maps to unauthorized`, async () => {
    const boat = new BoatProvider({ baseUrl: "https://boat.invalid", apiToken: "k" }, {
      fetch: fetchAnswering(status, '{"code":"unauthorized"}'),
    });
    const err = await assertRejects(() => boat.getInstance("sb_1"), ProviderError);
    assertEquals(err.kind, "unauthorized");
  });
  Deno.test(`Daytona ${status} maps to unauthorized`, async () => {
    const daytona = new DaytonaProvider({ baseUrl: "https://daytona.invalid", apiKey: "k" }, {
      fetch: fetchAnswering(status),
    });
    const err = await assertRejects(() => daytona.getInstance("sb_1"), ProviderError);
    assertEquals(err.kind, "unauthorized");
  });
}

Deno.test("Boat's plan-limit 403 stays quota_exceeded", async () => {
  const boat = new BoatProvider({ baseUrl: "https://boat.invalid", apiToken: "k" }, {
    fetch: fetchAnswering(403, '{"code":"machine_class_plan_required"}'),
  });
  const err = await assertRejects(() => boat.getInstance("sb_1"), ProviderError);
  assertEquals(err.kind, "quota_exceeded");
});

Deno.test("validateCredentials makes one read-only call with the key", async () => {
  const seen: Request[] = [];
  const boat = new BoatProvider({ baseUrl: "https://boat.invalid/api/v1", apiToken: "user-key" }, {
    fetch: fetchAnswering(200, '{"sandboxes":[]}', seen),
  });
  await defined(boat.validateCredentials).call(boat);
  assertEquals(seen.length, 1);
  assertEquals(seen[0].method, "GET");
  assertEquals(seen[0].headers.get("authorization"), "Bearer user-key");

  const dseen: Request[] = [];
  const daytona = new DaytonaProvider({ baseUrl: "https://daytona.invalid", apiKey: "dk" }, {
    fetch: fetchAnswering(200, "[]", dseen),
  });
  await defined(daytona.validateCredentials).call(daytona);
  assertEquals(dseen[0].method, "GET");
  assertEquals(dseen[0].headers.get("authorization"), "Bearer dk");
});

Deno.test("validateCredentials rejects a revoked key as unauthorized", async () => {
  const boat = new BoatProvider({ baseUrl: "https://boat.invalid", apiToken: "bad" }, {
    fetch: fetchAnswering(401),
  });
  const err = await assertRejects(() => defined(boat.validateCredentials).call(boat), ProviderError);
  assertEquals(err.kind, "unauthorized");
});

// ── Registry with a credential ─────────────────────────────────────────

Deno.test("a credential replaces the env secret in the adapter config", () => {
  assertEquals(boatConfigFromEnv(envOf({}), { apiKey: "user-boat" }).apiToken, "user-boat");
  assertEquals(
    boatConfigFromEnv(envOf({ TREQ_BOAT_API_URL: "https://b.invalid", TREQ_BOAT_API_TOKEN: "treq" }), {
      apiKey: "user-boat",
    }),
    { baseUrl: "https://b.invalid", apiToken: "user-boat", autoStopMinutes: 60 },
  );
  assertEquals(daytonaConfigFromEnv(envOf({}), { apiKey: "user-dt" }).apiKey, "user-dt");
});

Deno.test("providerFor builds the real adapter from a credential with no env secret", () => {
  assertInstanceOf(providerFor("boat", envOf({}), { apiKey: "user-boat" }), BoatProvider);
  assertInstanceOf(providerFor("daytona", envOf({}), { apiKey: "user-dt" }), DaytonaProvider);
});

Deno.test("providerFor keeps the stub under TREQ_REMOTE_PROVIDER_STUB, credential or not", () => {
  const p = providerFor("boat", envOf({ TREQ_REMOTE_PROVIDER_STUB: "1" }), { apiKey: "x" });
  assertInstanceOf(p, LocalProvider);
});

// ── providerForPlacement ───────────────────────────────────────────────

function fakeCredentialStore(
  rows: Record<string, { credential_source: "managed" | "byos"; credential_id: string | null; key: string | null }>,
) {
  const invalid: string[] = [];
  const loads: string[] = [];
  const store: Pick<ByosCredentialStore, "loadForPlacement" | "markInvalid"> = {
    loadForPlacement: (placementId) => {
      loads.push(placementId);
      return Promise.resolve(rows[placementId] ?? null);
    },
    markInvalid: (id, key) => {
      invalid.push(`${id}:${key}`);
      return Promise.resolve();
    },
  };
  return { store, invalid, loads };
}

function recordingBuild(fail?: ProviderError) {
  const built: Array<{ kind: ProviderKind; key: string | null }> = [];
  const build = (kind: ProviderKind, credential?: { apiKey: string }): SandboxProvider => {
    built.push({ kind, key: credential?.apiKey ?? null });
    const p = new LocalProvider(kind);
    if (fail) p.getInstance = () => Promise.reject(fail);
    return p;
  };
  return { build, built };
}

Deno.test("a managed placement uses treq's own account", async () => {
  const { store } = fakeCredentialStore({ p1: { credential_source: "managed", credential_id: null, key: null } });
  const { build, built } = recordingBuild();
  const provider = providerForPlacement(store, "boat", "p1", build);
  assertEquals(provider.kind, "boat");
  await assertRejects(() => provider.getInstance("missing"), ProviderError);
  assertEquals(built, [{ kind: "boat", key: null }]);
});

Deno.test("a BYOS placement builds its adapter with the owner's key, once", async () => {
  const { store, loads } = fakeCredentialStore({ p2: { credential_source: "byos", credential_id: "c2", key: "user-key" } });
  const { build, built } = recordingBuild();
  const provider = providerForPlacement(store, "daytona", "p2", build);
  await assertRejects(() => provider.getInstance("a"), ProviderError);
  await assertRejects(() => provider.getInstance("b"), ProviderError);
  assertEquals(built, [{ kind: "daytona", key: "user-key" }]);
  assertEquals(loads, ["p2"]);
});

Deno.test("no placement id means treq's account and no lookup", async () => {
  const { store, loads } = fakeCredentialStore({});
  const { build, built } = recordingBuild();
  await assertRejects(() => providerForPlacement(store, "boat", null, build).getInstance("x"), ProviderError);
  assertEquals(loads, []);
  assertEquals(built, [{ kind: "boat", key: null }]);
});

Deno.test("a BYOS placement whose key is gone fails as unauthorized, never falls back", async () => {
  const { store } = fakeCredentialStore({ p3: { credential_source: "byos", credential_id: null, key: null } });
  const { build, built } = recordingBuild();
  const err = await assertRejects(() => providerForPlacement(store, "boat", "p3", build).getInstance("x"), ProviderError);
  assertEquals(err.kind, "unauthorized");
  assertEquals(built, []);
});

Deno.test("an unauthorized answer marks the credential invalid and is rethrown", async () => {
  const { store, invalid } = fakeCredentialStore({ p4: { credential_source: "byos", credential_id: "c4", key: "k" } });
  const { build } = recordingBuild(new ProviderError("unauthorized", "revoked"));
  const err = await assertRejects(() => providerForPlacement(store, "boat", "p4", build).getInstance("x"), ProviderError);
  assertEquals(err.kind, "unauthorized");
  // The key that failed goes along, so a 401 racing a rotation spares the new key.
  assertEquals(invalid, ["c4:k"]);
});

Deno.test("unauthorized surfaces to the app as provider_credential_invalid", () => {
  assertEquals(providerErrorCode(new ProviderError("unauthorized", "x")), "provider_credential_invalid");
  assertEquals(providerErrorCode(new ProviderError("timeout", "x")), "timeout");
});

// ── The gate ──────────────────────────────────────────────

// ── Credential actions ─────────────────────────────────────────────────

function fakeActionStore() {
  const saved: Array<{ kind: string; key: string; label: string | null }> = [];
  const deleted: string[] = [];
  const store: ByosCredentialStore = {
    loadForPlacement: () => Promise.resolve(null),
    markInvalid: () => Promise.resolve(),
    save: (kind, key, label) => {
      saved.push({ kind, key, label });
      return Promise.resolve({ id: "c1", provider_kind: kind, label, validated_at: "now", invalid_at: null });
    },
    list: () => Promise.resolve([{ id: "c1", provider_kind: "boat", label: null, validated_at: "now", invalid_at: null }]),
    remove: (kind) => {
      deleted.push(kind);
      return Promise.resolve();
    },
    byosCatalog: () => Promise.resolve([]),
  };
  return { store, saved, deleted };
}

function validatingProvider(result: "ok" | "unauthorized") {
  return (kind: ProviderKind, credential?: { apiKey: string }): SandboxProvider => {
    // Like providerFor: an unknown kind has no adapter.
    if (!["boat", "daytona", "local"].includes(kind)) throw new ProviderError("invalid_request", "unknown kind");
    const p = new LocalProvider(kind);
    p.validateCredentials = () => {
      assertEquals(credential?.apiKey, "sk-user");
      return result === "ok" ? Promise.resolve() : Promise.reject(new ProviderError("unauthorized", "bad key"));
    };
    return p;
  };
}

Deno.test("saving a key validates it with the provider, then stores it", async () => {
  const { store, saved } = fakeActionStore();
  const r = await handleByosAction("save_provider_credential", { provider_kind: "boat", api_key: "sk-user", label: "Mine" }, {
    canByos: () => Promise.resolve(true),
    store,
    build: validatingProvider("ok"),
  });
  assertEquals(r?.status, 200);
  assertEquals(saved, [{ kind: "boat", key: "sk-user", label: "Mine" }]);
  assert(!JSON.stringify(r?.body).includes("sk-user"), "the key is never echoed");
});

Deno.test("an invalid key is refused and not stored", async () => {
  const { store, saved } = fakeActionStore();
  const r = await handleByosAction("save_provider_credential", { provider_kind: "daytona", api_key: "sk-user" }, {
    canByos: () => Promise.resolve(true),
    store,
    build: validatingProvider("unauthorized"),
  });
  assertEquals(r?.status, 400);
  assertEquals((r?.body as { code: string }).code, "provider_credential_invalid");
  assertEquals(saved, []);
});

Deno.test("a user the gate refuses cannot save a key", async () => {
  const { store, saved } = fakeActionStore();
  const r = await handleByosAction("save_provider_credential", { provider_kind: "boat", api_key: "sk-user" }, {
    canByos: () => Promise.resolve(false),
    store,
    build: validatingProvider("ok"),
  });
  assertEquals(r?.status, 403);
  assertEquals((r?.body as { code: string }).code, "byos_not_allowed");
  assertEquals(saved, []);
});

Deno.test("the local stub and unknown kinds take no key", async () => {
  const { store } = fakeActionStore();
  for (const kind of ["local", "nope", 3]) {
    const r = await handleByosAction("save_provider_credential", { provider_kind: kind, api_key: "sk-user" }, {
      canByos: () => Promise.resolve(true),
      store,
      build: validatingProvider("ok"),
    });
    assertEquals(r?.status, 400);
  }
});

Deno.test("non-BYOS actions pass through", async () => {
  const { store } = fakeActionStore();
  assertEquals(
    await handleByosAction("ensure", {}, { canByos: () => Promise.resolve(true), store, build: validatingProvider("ok") }),
    null,
  );
});

// ── Placement choice for ensure and resize ─────────────────────────────

const TYPES = [
  { id: "daytona:s", provider_kind: "daytona" as ProviderKind, vendor_type: "s", tier_id: "s", regions: ["us_east"], selectable: true },
  { id: "daytona:m", provider_kind: "daytona" as ProviderKind, vendor_type: "m", tier_id: "m", regions: ["us_east"], selectable: false },
  { id: "boat:default", provider_kind: "boat" as ProviderKind, vendor_type: "default", tier_id: "m", regions: ["us_east"], selectable: true },
];

Deno.test("the placement choice defaults to treq managed", () => {
  assertEquals(parsePlacementChoice({}), { ok: true, value: { mode: "managed" } });
  assertEquals(parsePlacementChoice({ credential_source: "managed" }), { ok: true, value: { mode: "managed" } });
  assertEquals(parsePlacementChoice({ credential_source: "byos", provider_kind: "daytona" }), {
    ok: true,
    value: { mode: "byos", kind: "daytona" },
  });
  assertEquals(parsePlacementChoice({ credential_source: "byos" }).ok, false);
  assertEquals(parsePlacementChoice({ credential_source: "other" }).ok, false);
});

function planDeps(over: Partial<Parameters<typeof planByosPlacement>[0]> = {}) {
  return {
    canByos: () => Promise.resolve(true),
    credentialFor: (kind: ProviderKind) =>
      Promise.resolve(kind === "daytona" ? { id: "c-dt", invalid_at: null } : null),
    machineTypes: (kind: ProviderKind) => Promise.resolve(TYPES.filter((t) => t.provider_kind === kind)),
    ...over,
  };
}

Deno.test("ensure in BYOS mode picks the user's provider for the tier, ignoring the cheapest default", async () => {
  const plan = await planByosPlacement(planDeps(), { mode: "byos", kind: "daytona" }, "m", "us_east");
  // daytona:m is hidden from the managed catalog, but a BYOS user may still run it.
  assertEquals(plan, { ok: true, machineType: TYPES[1], credentialId: "c-dt" });
});

Deno.test("BYOS needs a saved key for that provider", async () => {
  const plan = await planByosPlacement(planDeps(), { mode: "byos", kind: "boat" }, "m", "us_east");
  assertEquals(plan, { ok: false, status: 400, code: "byos_credential_missing" });
});

Deno.test("BYOS with a key marked invalid is refused up front", async () => {
  const plan = await planByosPlacement(
    planDeps({ credentialFor: () => Promise.resolve({ id: "c", invalid_at: "2026-10-28T00:00:00Z" }) }),
    { mode: "byos", kind: "daytona" },
    "m",
    "us_east",
  );
  assertEquals(plan, { ok: false, status: 409, code: "provider_credential_invalid" });
});

Deno.test("BYOS is refused when gated", async () => {
  assertEquals(
    await planByosPlacement(planDeps({ canByos: () => Promise.resolve(false) }), { mode: "byos", kind: "daytona" }, "m", "us_east"),
    { ok: false, status: 403, code: "byos_not_allowed" },
  );
});

Deno.test("BYOS with no machine type for the tier or region is tier_unavailable", async () => {
  assertEquals(
    await planByosPlacement(planDeps(), { mode: "byos", kind: "daytona" }, "xl", "us_east"),
    { ok: false, status: 400, code: "tier_unavailable" },
  );
  assertEquals(
    await planByosPlacement(planDeps(), { mode: "byos", kind: "daytona" }, "s", "eu_west"),
    { ok: false, status: 400, code: "tier_unavailable" },
  );
});

// ── Operator moves ─────────────────────────────────────────────────────

Deno.test("operator drains and cheaper-default moves skip BYOS placements", () => {
  assertEquals(isOperatorMovable({ credential_source: "managed" }), true);
  assertEquals(isOperatorMovable({ credential_source: "byos" }), false);
});
