/**
 * Volume pricing (072): build_usage_reports bills each hour at the tier the
 * invoice owner's usage this period has reached. Tiers are 730 h of a Medium
 * (4 vCPU) sandbox = 10,512,000 vCPU-seconds; managed markup 20/15/10%, BYOS
 * fee 5/4/3%.
 */
import { randomUUID } from "node:crypto";
import { afterAll, beforeAll, expect, it } from "vitest";
import { clearBilling, grantPro } from "../billing";
import { getServiceClient } from "../clients";
import { recordOutcome } from "../record";
import { createTestUser, deleteTestUser } from "../seed";

const admin = getServiceClient();
const TIER = 10_512_000;
const HOUR = 3_600_000;
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

/** A Pro user whose period started 2 days ago, with prior usage this period. */
async function proUser(prior: { managed: number; byos: number }) {
  const { user } = await createTestUser();
  users.push(user.id);
  const grant = await grantPro(admin, user.id);
  const { error: periodError } = await admin
    .from("billing_subscriptions")
    .update({
      current_period_start: new Date(Date.now() - 2 * 86_400_000).toISOString(),
    })
    .eq("stripe_subscription_id", grant.subscriptionId);
  if (periodError) throw new Error(periodError.message);
  if (prior.managed || prior.byos) {
    const { error } = await admin.from("usage_reports").insert({
      identifier: `vp-prior-${user.id}`,
      owner_user_id: user.id,
      hour: new Date(
        Math.floor((Date.now() - 86_400_000) / HOUR) * HOUR,
      ).toISOString(),
      seconds: 0,
      provider_cost_nanousd: 0,
      billed_nanousd: 0,
      managed_vcpu_seconds: prior.managed,
      byos_vcpu_seconds: prior.byos,
      sent_at: new Date().toISOString(),
    });
    if (error) throw new Error(error.message);
  }
  return user.id;
}

/** A workspace running for the last 3 hours, managed or on the owner's key. */
async function running(owner: string, byos: boolean) {
  const instanceId = randomUUID();
  const placementId = randomUUID();
  const credentialId = byos
    ? (
        await rpc<{ id: string }>("provider_credential_save", {
          p_user_id: owner,
          p_kind: "e2b",
          p_key: `k-${owner.slice(0, 8)}`,
          p_label: null,
        })
      ).id
    : null;
  const { error: ie } = await admin.from("remote_instances").insert({
    id: instanceId,
    owner_user_id: owner,
    provider_kind: "e2b",
    provider_resource_id: "vp",
    region: "us_east",
    size_preset: "small",
    status: "provisioning",
    tier_id: "s",
    machine_type_id: "e2b:s",
  });
  if (ie) throw new Error(ie.message);
  const { error: pe } = await admin.from("remote_instance_placements").insert({
    id: placementId,
    instance_id: instanceId,
    owner_user_id: owner,
    provider_kind: "e2b",
    machine_type_id: "e2b:s",
    region: "us_east",
    provider_resource_id: `vp-${placementId.slice(0, 8)}`,
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
  identifier: string;
  provider_cost_nanousd: number;
  billed_nanousd: number;
  byos_billed_nanousd: number;
  managed_vcpu_seconds: number;
  byos_vcpu_seconds: number;
};

async function reports(owner: string): Promise<Report[]> {
  const { data, error } = await admin
    .from("usage_reports")
    .select(
      "identifier, provider_cost_nanousd, billed_nanousd, byos_billed_nanousd, managed_vcpu_seconds, byos_vcpu_seconds",
    )
    .eq("owner_user_id", owner)
    .not("identifier", "like", "vp-prior-%");
  if (error) throw new Error(error.message);
  return (data ?? []) as Report[];
}

const near = (a: number, b: number) => Math.abs(a - b) <= 1;

it("bills each hour at the owner's volume tier", async () => {
  const light = await proUser({ managed: 0, byos: 0 });
  const heavy = await proUser({ managed: 2 * TIER, byos: 0 });
  const byosUser = await proUser({ managed: 0, byos: TIER });
  await running(light, false);
  await running(heavy, false);
  await running(byosUser, true);

  await rpc("build_usage_reports", { p_until: new Date().toISOString() });

  const l = await reports(light);
  const h = await reports(heavy);
  const b = await reports(byosUser);
  expect(l.length).toBeGreaterThanOrEqual(2);
  expect(h.length).toBeGreaterThanOrEqual(2);
  expect(b.length).toBeGreaterThanOrEqual(2);
  for (const r of l)
    expect(
      near(r.billed_nanousd, Math.round(r.provider_cost_nanousd * 1.2)),
    ).toBe(true);
  for (const r of h)
    expect(
      near(r.billed_nanousd, Math.round(r.provider_cost_nanousd * 1.1)),
    ).toBe(true);
  for (const r of b) {
    expect(
      near(r.byos_billed_nanousd, Math.round(r.provider_cost_nanousd * 0.04)),
    ).toBe(true);
    expect(r.billed_nanousd).toBe(r.byos_billed_nanousd);
    expect(r.byos_vcpu_seconds).toBeGreaterThan(0);
    expect(r.managed_vcpu_seconds).toBe(0);
  }
  expect(h.every((r) => r.managed_vcpu_seconds > 0)).toBe(true);

  await recordOutcome("volume-pricing-01-tiers", {
    expectations: [
      "A Pro user with no usage this period pays provider cost plus 20%.",
      "A Pro user past two sandbox-months of managed usage this period pays cost plus 10%.",
      "A BYOS user in the second sandbox-month pays a 4% fee, counted apart from managed usage.",
    ],
    details: { light: l, heavy: h, byos: b },
  });
}, 120_000);
