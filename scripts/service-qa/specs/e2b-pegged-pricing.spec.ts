/**
 * E2B-pegged pricing (072): running time bills at E2B's rate for the size,
 * whichever provider runs it. Managed pays it plus 20%, BYOS pays 5% of it.
 * usage_reports keeps the real provider cost.
 */
import { randomUUID } from "node:crypto";
import { afterAll, beforeAll, expect, it } from "vitest";
import { clearBilling, grantPro } from "../billing";
import { getAnonClient, getServiceClient } from "../clients";
import { recordOutcome } from "../record";
import { createTestUser, deleteTestUser } from "../seed";

const admin = getServiceClient();
const HOUR = 3_600_000;
const E2B_MEDIUM = 92_000; // nano-USD per second
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
async function boatMedium(owner: string, byos: boolean) {
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
    p_state: "ready",
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

it("bills Boat usage at E2B's price for the size", async () => {
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
      Math.abs(r.billed_nanousd - r.seconds * E2B_MEDIUM * 1.2),
    ).toBeLessThanOrEqual(E2B_MEDIUM * 1.2);
  }
  for (const r of b) {
    expect(
      Math.abs(r.byos_billed_nanousd - r.seconds * E2B_MEDIUM * 0.05),
    ).toBeLessThanOrEqual(E2B_MEDIUM * 0.05);
    expect(r.billed_nanousd).toBe(r.byos_billed_nanousd);
  }

  await recordOutcome("e2b-pegged-pricing-01-usage", {
    expectations: [
      "Managed Boat Medium bills E2B Medium + 20% per second (US$0.39744/h), not Boat's cost + 20%.",
      "BYOS on a Boat key bills 5% of E2B Medium (US$0.01656/h).",
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
  expect(prices.m).toEqual([0.39744]);

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
  expect(Number(boatM?.hourly_usd)).toBe(0.01656);
  expect(Number(boatM?.provider_hourly_usd)).toBe(0.036);

  await recordOutcome("e2b-pegged-pricing-02-catalog", {
    expectations: [
      "A signed-in user sees one Medium price (US$0.39744/h) in every region of get_machine_catalog.",
      "get_byos_catalog shows a Boat Medium fee of 5% of E2B Medium (US$0.01656/h) next to Boat's own US$0.036/h.",
    ],
    details: { prices, boatM },
  });
}, 120_000);
