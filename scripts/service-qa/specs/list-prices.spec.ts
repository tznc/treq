/**
 * Fixed list prices (072): one price per size on every provider. Medium bills
 * US$0.40/h managed and US$0.017/h on a BYOS key; stopped disk bills
 * US$0.10/GB-month managed and US$0.004 BYOS. usage_reports keeps the real
 * provider cost.
 */
import { randomUUID } from "node:crypto";
import { afterAll, beforeAll, expect, it } from "vitest";
import { clearBilling, grantPro } from "../billing";
import { getAnonClient, getServiceClient } from "../clients";
import { recordOutcome } from "../record";
import { createTestUser, deleteTestUser } from "../seed";

const admin = getServiceClient();
const HOUR = 3_600_000;
const MANAGED_MEDIUM = 0.4e9 / 3600; // nano-USD per second
const BYOS_MEDIUM = 0.017e9 / 3600;
const BOAT_MEDIUM = 10_000;
const users: string[] = [];

beforeAll(async () => {
  const { error } = await admin
    .from("machine_type_prices")
    .update({
      effective_from: new Date(Date.now() - 30 * 86_400_000).toISOString(),
    })
    .is("effective_to", null)
    .gt("effective_from", new Date(Date.now() - 30 * 86_400_000).toISOString());
  if (error) throw new Error(error.message);
});

afterAll(async () => {
  await clearBilling(admin, users);
  for (const id of users) await deleteTestUser(admin, id);
});

async function rpc<T>(fn: string, args: Record<string, unknown>): Promise<T> {
  const { data, error } = await admin.rpc(fn, args);
  if (error) throw new Error(`${fn}: ${error.message}`);
  return data as T;
}

/** A Boat Medium workspace running for the last 3 hours. */
async function boatMedium(
  owner: string,
  byos: boolean,
  state = "ready",
  diskGb: number | null = null,
) {
  const instanceId = randomUUID();
  const placementId = randomUUID();
  const credentialId = byos
    ? (
        await rpc<{ id: string }>("provider_credential_save", {
          p_user_id: owner,
          p_kind: "boat",
          p_key: `k-${owner.slice(0, 8)}`,
          p_label: null,
        })
      ).id
    : null;
  const { error: ie } = await admin.from("remote_instances").insert({
    id: instanceId,
    owner_user_id: owner,
    provider_kind: "boat",
    provider_resource_id: "pegged",
    region: "eu_west",
    size_preset: "medium",
    status: "provisioning",
    tier_id: "m",
    machine_type_id: "boat:default",
  });
  if (ie) throw new Error(ie.message);
  const { error: pe } = await admin.from("remote_instance_placements").insert({
    id: placementId,
    instance_id: instanceId,
    owner_user_id: owner,
    provider_kind: "boat",
    machine_type_id: "boat:default",
    region: "eu_west",
    provider_resource_id: `pegged-${placementId.slice(0, 8)}`,
    state: "provisioning",
    role: "active",
    credential_source: byos ? "byos" : "managed",
    credential_id: credentialId,
  });
  if (pe) throw new Error(pe.message);
  await rpc("set_placement_state", {
    p_placement_id: placementId,
    p_state: state,
    p_disk_gb: diskGb,
    p_at: new Date(Date.now() - 3 * HOUR).toISOString(),
  });
}

type Report = {
  seconds: number;
  provider_cost_nanousd: number;
  billed_nanousd: number;
  byos_billed_nanousd: number;
};

async function reports(owner: string): Promise<Report[]> {
  const { data, error } = await admin
    .from("usage_reports")
    .select(
      "seconds, provider_cost_nanousd, billed_nanousd, byos_billed_nanousd",
    )
    .eq("owner_user_id", owner);
  if (error) throw new Error(error.message);
  return (data ?? []) as Report[];
}

it("bills Boat usage at the list price for the size", async () => {
  const managed = await createTestUser();
  const byos = await createTestUser();
  users.push(managed.user.id, byos.user.id);
  await grantPro(admin, managed.user.id);
  await grantPro(admin, byos.user.id);
  await boatMedium(managed.user.id, false);
  await boatMedium(byos.user.id, true);

  await rpc("build_usage_reports", { p_until: new Date().toISOString() });

  const m = await reports(managed.user.id);
  const b = await reports(byos.user.id);
  expect(m.length).toBeGreaterThanOrEqual(2);
  expect(b.length).toBeGreaterThanOrEqual(2);
  for (const r of m) {
    expect(
      Math.abs(r.provider_cost_nanousd - r.seconds * BOAT_MEDIUM),
    ).toBeLessThanOrEqual(BOAT_MEDIUM);
    expect(
      Math.abs(r.billed_nanousd - r.seconds * MANAGED_MEDIUM),
    ).toBeLessThanOrEqual(MANAGED_MEDIUM);
  }
  for (const r of b) {
    expect(
      Math.abs(r.byos_billed_nanousd - r.seconds * BYOS_MEDIUM),
    ).toBeLessThanOrEqual(BYOS_MEDIUM);
    expect(r.billed_nanousd).toBe(r.byos_billed_nanousd);
  }

  await recordOutcome("list-prices-01-usage", {
    expectations: [
      "Managed Boat Medium bills the Medium list price, US$0.40/h, not Boat's cost + 20%.",
      "BYOS on a Boat key bills the BYOS Medium list price, US$0.017/h.",
      "provider_cost_nanousd keeps Boat's real cost (US$0.036/h).",
    ],
    details: { managed: m, byos: b },
  });
}, 120_000);

it("shows one price per size in the catalogs", async () => {
  const u = await createTestUser();
  users.push(u.user.id);
  await grantPro(admin, u.user.id);
  await rpc("provider_credential_save", {
    p_user_id: u.user.id,
    p_kind: "boat",
    p_key: `k-${u.user.id.slice(0, 8)}`,
    p_label: null,
  });
  const anon = getAnonClient();
  const { error: signInError } = await anon.auth.signInWithPassword({
    email: u.email,
    password: u.password,
  });
  expect(signInError).toBeNull();

  const { data: catalog, error } = await anon.rpc("get_machine_catalog");
  expect(error).toBeNull();
  const tiers = catalog as { id: string; regions: { hourly_usd: number }[] }[];
  const prices = Object.fromEntries(
    tiers.map((t) => [
      t.id,
      [...new Set(t.regions.map((r) => Number(r.hourly_usd)))],
    ]),
  );
  expect(prices.m).toEqual([0.4]);

  const { data: byosCatalog, error: byosError } =
    await anon.rpc("get_byos_catalog");
  expect(byosError).toBeNull();
  const boatM = (
    byosCatalog as {
      tier_id: string;
      hourly_usd: number;
      provider_hourly_usd: number;
    }[]
  ).find((e) => e.tier_id === "m");
  expect(Number(boatM?.hourly_usd)).toBe(0.017);
  expect(Number(boatM?.provider_hourly_usd)).toBe(0.036);

  await recordOutcome("list-prices-02-catalog", {
    expectations: [
      "A signed-in user sees one Medium price (US$0.40/h) in every region of get_machine_catalog.",
      "get_byos_catalog shows a Boat Medium fee of US$0.017/h next to Boat's own US$0.036/h.",
    ],
    details: { prices, boatM },
  });
}, 120_000);

it("bills stopped Boat disk at the list price per GB", async () => {
  const DISK_MANAGED = 0.1e9 / (730 * 3600); // nano-USD per GB-second
  const DISK_BYOS = 0.004e9 / (730 * 3600);
  const managed = await createTestUser();
  const byos = await createTestUser();
  users.push(managed.user.id, byos.user.id);
  await grantPro(admin, managed.user.id);
  await grantPro(admin, byos.user.id);
  await boatMedium(managed.user.id, false, "suspended", 10);
  await boatMedium(byos.user.id, true, "suspended", 10);

  await rpc("build_usage_reports", { p_until: new Date().toISOString() });

  const m = await reports(managed.user.id);
  const b = await reports(byos.user.id);
  expect(m.length).toBeGreaterThanOrEqual(2);
  expect(b.length).toBeGreaterThanOrEqual(2);

  for (const r of m) {
    expect(r.provider_cost_nanousd).toBe(0);
    expect(
      Math.abs(r.billed_nanousd - r.seconds * DISK_MANAGED * 10),
    ).toBeLessThanOrEqual(DISK_MANAGED * 10);
  }
  for (const r of b) {
    expect(
      Math.abs(r.byos_billed_nanousd - r.seconds * DISK_BYOS * 10),
    ).toBeLessThanOrEqual(DISK_BYOS * 10 + 1);
  }

  await recordOutcome("list-prices-03-stopped-disk", {
    expectations: [
      "A suspended managed Boat workspace with 10 GB bills US$0.10/GB-month, though Boat charges Treq nothing.",
      "A suspended BYOS Boat workspace with 10 GB bills US$0.004/GB-month.",
    ],
    details: { managed: m, byos: b },
  });
}, 120_000);

it("takes a price edit from treq_admin.admin_config without a migration", async () => {
  const u = await createTestUser();
  users.push(u.user.id);
  await grantPro(admin, u.user.id);
  const anon = getAnonClient();
  const { error: signInError } = await anon.auth.signInWithPassword({
    email: u.email,
    password: u.password,
  });
  expect(signInError).toBeNull();

  const { data: row, error: readError } = await admin.rpc("admin_config_get", {
    p_key: "list_prices",
  });
  expect(readError).toBeNull();
  const original = row as Record<string, unknown>;
  const edited = structuredClone(original) as {
    tiers: Record<string, { managed_usd_per_hour: number }>;
  };
  edited.tiers.m.managed_usd_per_hour = 0.36;

  try {
    const { error: editError } = await admin.rpc("admin_config_set", {
      p_key: "list_prices",
      p_value: edited,
    });
    expect(editError).toBeNull();
    const { data: catalog } = await anon.rpc("get_machine_catalog");
    const medium = (
      catalog as { id: string; regions: { hourly_usd: number }[] }[]
    ).find((t) => t.id === "m");
    expect([
      ...new Set(medium?.regions.map((r) => Number(r.hourly_usd))),
    ]).toEqual([0.36]);

    const { error: badError } = await admin.rpc("admin_config_set", {
      p_key: "list_prices",
      p_value: { tiers: {} },
    });
    expect(badError?.message).toContain(
      "admin_config: list_prices does not match its schema",
    );

    const { error: anonError } = await anon.rpc("admin_config_get", {
      p_key: "list_prices",
    });
    expect(anonError).not.toBeNull();
    const { error: tableError } = await anon
      .schema("treq_admin")
      .from("admin_config")
      .select("key");
    expect(tableError).not.toBeNull();

    await recordOutcome("list-prices-04-admin-config-edit", {
      expectations: [
        "Setting the list_prices key through admin_config_set (service role) changes the Medium catalog price to US$0.36/h at once.",
        "A value that drops the tier prices is rejected by its JSON Schema, naming the key.",
        "A signed-in user can neither call admin_config_get nor query the treq_admin schema.",
      ],
      details: {
        medium,
        badError: badError?.message,
        anonError: anonError?.message ?? null,
        tableError: tableError?.message ?? null,
      },
    });
  } finally {
    await admin.rpc("admin_config_set", {
      p_key: "list_prices",
      p_value: original,
    });
  }
}, 120_000);
