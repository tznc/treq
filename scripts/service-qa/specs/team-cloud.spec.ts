/**
 * Team cloud (068_team_cloud.sql) against the local stack: an organization
 * owner's cloud settings, a member's profile, organization BYOS keys, usage
 * billed to the organization's customer, and the organization and member
 * spend caps. remote-instance runs in-process with the service-role client
 * and each caller's own signed-in client, wired as its index.ts wires them.
 */
import { randomUUID } from "node:crypto";
import type { SupabaseClient } from "@supabase/supabase-js";
import { beforeAll, expect, it } from "vitest";
import { userHasCloud } from "../../../supabase/functions/_shared/billing/entitlement";
import { spendStatusFor } from "../../../supabase/functions/_shared/billing/spend-cap";
import {
  reportUsage,
  supabaseUsageReportStore,
} from "../../../supabase/functions/_shared/billing/usage-report";
import { supabaseByosStore } from "../../../supabase/functions/_shared/remote/byos";
import type { CloudContext } from "../../../supabase/functions/_shared/remote/instance-actions";
import { LocalProvider } from "../../../supabase/functions/_shared/remote/local-provider";
import type { ProviderKind } from "../../../supabase/functions/_shared/remote/provider";
import { providerFor } from "../../../supabase/functions/_shared/remote/provider-registry";
import { handleRemoteInstanceAction } from "../../../supabase/functions/remote-instance/lib";
import { clearBilling, grantTeam } from "../billing";
import { getServiceClient } from "../clients";
import { recordOutcome } from "../record";
import {
  createTestUser,
  deleteTestUser,
  signInWithEmailPassword,
  type TestUser,
} from "../seed";

const admin = getServiceClient();
const stubEnv = (k: string) =>
  k === "TREQ_REMOTE_PROVIDER_STUB" ? "1" : undefined;

beforeAll(async () => {
  // Seed prices start at db reset; usage below is backdated by hours.
  const { error } = await admin
    .from("machine_type_prices")
    .update({
      effective_from: new Date(Date.now() - 30 * 86_400_000).toISOString(),
    })
    .is("effective_to", null)
    .gt("effective_from", new Date(Date.now() - 30 * 86_400_000).toISOString());
  if (error) throw new Error(error.message);
});

type Caller = { id: string; client: SupabaseClient };

async function signIn(u: TestUser): Promise<Caller> {
  const { client } = await signInWithEmailPassword(u.email, u.password);
  return { id: u.user.id, client };
}

async function call(
  caller: Caller,
  action: string,
  body: Record<string, unknown> = {},
) {
  const response = await handleRemoteInstanceAction(
    { action, idempotency_key: `${action}-${randomUUID()}`, ...body },
    {
      supabase: admin,
      ownerUserId: caller.id,
      correlationId: `sqa-${randomUUID()}`,
      hasPro: () => userHasCloud(admin, caller.id),
      cloudContext: async () => {
        const { data, error } = await admin.rpc("cloud_context_for", {
          p_user_id: caller.id,
        });
        if (error) throw new Error(error.message);
        return data as CloudContext;
      },
      spendStatus: () => spendStatusFor(admin, caller.id),
      stub: true,
      provider: (kind) => new LocalProvider(kind),
      byos: {
        canByos: async () =>
          (await admin.rpc("can_byos", { p_user_id: caller.id })).data === true,
        store: supabaseByosStore(admin, caller.client, caller.id),
        build: (kind: ProviderKind, credential?: { apiKey: string }) =>
          providerFor(kind, stubEnv, credential),
      },
    },
  );
  return {
    status: response.status,
    json: (await response.json()) as Record<string, unknown>,
  };
}

async function team() {
  const ownerU = await createTestUser();
  const memberU = await createTestUser();
  const { data: org, error } = await admin
    .from("organizations")
    .insert({ name: "SQA team cloud" })
    .select("id")
    .single();
  if (error) throw new Error(error.message);
  const orgId = (org as { id: string }).id;
  await admin.from("organization_members").insert([
    { org_id: orgId, user_id: ownerU.user.id, role: "owner" },
    { org_id: orgId, user_id: memberU.user.id, role: "member" },
  ]);
  const grant = await grantTeam(admin, orgId);
  return {
    orgId,
    grant,
    owner: await signIn(ownerU),
    member: await signIn(memberU),
    users: [ownerU, memberU],
  };
}

async function cleanup(t: Awaited<ReturnType<typeof team>>) {
  for (const u of t.users) {
    await admin.from("usage_reports").delete().eq("owner_user_id", u.user.id);
    await admin
      .from("remote_instances")
      .delete()
      .eq("owner_user_id", u.user.id);
  }
  await admin
    .from("remote_provider_credentials")
    .delete()
    .eq("owner_org_id", t.orgId);
  await clearBilling(admin, [t.orgId]);
  await admin.from("organizations").delete().eq("id", t.orgId);
  for (const u of t.users) await deleteTestUser(admin, u.user.id);
}

it("lets an owner choose managed, BYOS or both for members, who bill to the organization", async () => {
  const t = await team();
  try {
    // Off by default: a Team-only member has no cloud, personal or organization.
    const offPersonal = await call(t.member, "ensure");
    const profile = await t.member.client.rpc("set_cloud_profile", {
      p_org_id: t.orgId,
    });
    expect(profile.error).toBeNull();
    const offOrg = await call(t.member, "ensure");

    const memberSets = await t.member.client.rpc(
      "set_organization_cloud_settings",
      {
        p_org_id: t.orgId,
        p_allow_managed: true,
        p_allow_byos: true,
        p_org_cap_usd: 500,
        p_member_cap_usd: 50,
      },
    );
    const ownerSets = await t.owner.client.rpc(
      "set_organization_cloud_settings",
      {
        p_org_id: t.orgId,
        p_allow_managed: false,
        p_allow_byos: true,
        p_org_cap_usd: 500,
        p_member_cap_usd: 50,
      },
    );

    const managedRefused = await call(t.member, "ensure");
    const noKey = await call(t.member, "ensure", {
      credential_source: "byos",
      provider_kind: "boat",
      region: "eu_west",
    });
    const memberSavesKey = await call(
      t.member,
      "save_org_provider_credential",
      {
        org_id: t.orgId,
        provider_kind: "boat",
        api_key: "org-boat-key",
      },
    );
    const ownerSavesKey = await call(t.owner, "save_org_provider_credential", {
      org_id: t.orgId,
      provider_kind: "boat",
      api_key: "org-boat-key",
      label: "Team Boat",
    });
    const memberList = await call(t.member, "list_org_provider_credentials", {
      org_id: t.orgId,
    });
    const byos = await call(t.member, "ensure", {
      credential_source: "byos",
      provider_kind: "boat",
      region: "eu_west",
    });

    const { data: inst } = await admin
      .from("remote_instances")
      .select("id, billing_org_id, active_placement_id")
      .eq("owner_user_id", t.member.id)
      .single();
    const instance = inst as {
      id: string;
      billing_org_id: string;
      active_placement_id: string;
    };
    const { data: pl } = await admin
      .from("remote_instance_placements")
      .select("billing_org_id, credential_source, credential_id")
      .eq("id", instance.active_placement_id)
      .single();
    const loaded = await admin.rpc("provider_credential_load_for_placement", {
      p_placement_id: instance.active_placement_id,
    });
    const inUse = await call(t.owner, "delete_org_provider_credential", {
      org_id: t.orgId,
      provider_kind: "boat",
    });

    expect(offPersonal).toMatchObject({
      status: 402,
      json: { code: "pro_required" },
    });
    expect(offOrg).toMatchObject({
      status: 402,
      json: { code: "pro_required" },
    });
    expect(memberSets.error?.message).toContain("not_organization_owner");
    expect(ownerSets.error).toBeNull();
    expect(managedRefused).toMatchObject({
      status: 403,
      json: { code: "cloud_mode_not_allowed", mode: "managed" },
    });
    expect(noKey.json.code).toBe("byos_credential_missing");
    expect(memberSavesKey).toMatchObject({
      status: 403,
      json: { code: "not_organization_owner" },
    });
    expect(ownerSavesKey.status).toBe(200);
    expect(JSON.stringify(ownerSavesKey.json)).not.toContain("org-boat-key");
    expect((memberList.json.credentials as unknown[]).length).toBe(1);
    expect(byos.status, JSON.stringify(byos.json)).toBe(200);
    expect(instance.billing_org_id).toBe(t.orgId);
    expect(pl).toMatchObject({
      billing_org_id: t.orgId,
      credential_source: "byos",
    });
    expect(loaded.data).toMatchObject({
      credential_source: "byos",
      key: "org-boat-key",
    });
    expect(inUse).toMatchObject({
      status: 409,
      json: { code: "credential_in_use" },
    });

    await recordOutcome("team-cloud-01-settings-profile-keys", {
      expectations: [
        "With cloud off (the default), a Team-only member is refused on both the personal and the organization profile.",
        "Only the owner changes settings and organization keys; a member gets not_organization_owner.",
        "On the organization profile, managed is refused when only BYOS is allowed, and BYOS runs on the organization's key and bills to the organization.",
      ],
      details: {
        offPersonal: offPersonal.json,
        managedRefused: managedRefused.json,
        byos: byos.json,
        instance,
        placement: pl,
      },
    });
  } finally {
    await cleanup(t);
  }
}, 120000);

it("bills organization usage to the organization's customer and enforces both caps", async () => {
  const t = await team();
  try {
    await t.owner.client.rpc("set_organization_cloud_settings", {
      p_org_id: t.orgId,
      p_allow_managed: true,
      p_allow_byos: false,
      p_org_cap_usd: 500,
      p_member_cap_usd: 5,
    });
    await t.member.client.rpc("set_cloud_profile", { p_org_id: t.orgId });
    const ensured = await call(t.member, "ensure");
    expect(ensured.status, JSON.stringify(ensured.json)).toBe(200);
    const { data: inst } = await admin
      .from("remote_instances")
      .select("id, active_placement_id")
      .eq("owner_user_id", t.member.id)
      .single();
    const pid = (inst as { active_placement_id: string }).active_placement_id;
    // 20 hours of e2b:m managed (~$0.40/h billed) is past the $5 member cap.
    await admin
      .from("usage_intervals")
      .delete()
      .eq("owner_user_id", t.member.id);
    await admin
      .from("remote_instance_placements")
      .update({ machine_type_id: "e2b:m" })
      .eq("id", pid);
    await admin.rpc("set_placement_state", {
      p_placement_id: pid,
      p_state: "ready",
      p_at: new Date(Date.now() - 20 * 3600_000).toISOString(),
    });
    await admin.rpc("set_placement_state", {
      p_placement_id: pid,
      p_state: "suspended",
    });

    const status = await admin.rpc("get_spend_status_for", {
      p_user_id: t.member.id,
    });
    const wake = await call(t.member, "wake");

    const sent: Array<Record<string, unknown>> = [];
    const result = await reportUsage({
      store: supabaseUsageReportStore(admin),
      stripe: {
        request: async <T>(
          _m: string,
          _p: string,
          params?: Record<string, unknown>,
        ) => {
          if (params?.identifier && String(params.identifier).includes(t.orgId))
            sent.push(params);
          return {} as T;
        },
      },
      enabled: true,
      now: () => new Date(),
      log: () => {},
    });
    const { data: rows } = await admin
      .from("usage_reports")
      .select("identifier, billing_org_id, stripe_status")
      .eq("owner_user_id", t.member.id);

    expect(status.data).toMatchObject({
      scope: "organization",
      org_id: t.orgId,
      state: "capped",
      cap_usd: 5,
    });
    expect(wake).toMatchObject({
      status: 402,
      json: { code: "spend_cap_reached" },
    });
    expect((rows ?? []).length).toBeGreaterThan(0);
    for (const r of rows as Array<{
      billing_org_id: string;
      stripe_status: string;
    }>) {
      expect(r.billing_org_id).toBe(t.orgId);
      expect(r.stripe_status).toBe("accepted");
    }
    expect(sent.length).toBe((rows ?? []).length);
    for (const e of sent)
      expect(e["payload[stripe_customer_id]"]).toBe(t.grant.customerId);

    // Raising the member cap leaves the organization cap in force.
    await t.owner.client.rpc("set_organization_cloud_settings", {
      p_org_id: t.orgId,
      p_allow_managed: true,
      p_allow_byos: false,
      p_org_cap_usd: 5,
      p_member_cap_usd: 1000,
    });
    const orgCapped = await admin.rpc("get_spend_status_for", {
      p_user_id: t.member.id,
    });
    expect(orgCapped.data).toMatchObject({ state: "capped", org_cap_usd: 5 });

    await recordOutcome("team-cloud-02-billing-and-caps", {
      expectations: [
        "Every usage report of a member's organization workspace carries the organization and is sent to the organization's Stripe customer.",
        "Past the $5 member cap the member is capped (scope organization) and wake is refused with 402.",
        "With a large member cap, a $5 organization cap still caps the member.",
      ],
      details: {
        status: status.data,
        orgCapped: orgCapped.data,
        reports: rows,
        sentTo: sent.map((e) => e["payload[stripe_customer_id]"]),
        result,
      },
    });
  } finally {
    await cleanup(t);
  }
}, 120000);

it("keeps personal and organization access apart", async () => {
  const t = await team();
  const outsider = await signIn(await createTestUser());
  try {
    const outsiderProfile = await outsider.client.rpc("set_cloud_profile", {
      p_org_id: t.orgId,
    });
    const outsiderSettings = await outsider.client.rpc(
      "get_organization_cloud_settings",
      { p_org_id: t.orgId },
    );
    const outsiderKeys = await outsider.client.rpc(
      "list_org_provider_credentials",
      { p_org_id: t.orgId },
    );
    const memberPersonalKey = await call(t.member, "save_provider_credential", {
      provider_kind: "boat",
      api_key: "personal",
    });
    const tableRead = await t.member.client
      .from("organization_cloud_settings")
      .select("*");

    expect(outsiderProfile.error?.message).toContain("not_organization_member");
    expect(outsiderSettings.error?.message).toContain(
      "not_organization_member",
    );
    expect(outsiderKeys.data ?? []).toEqual([]);
    expect(memberPersonalKey).toMatchObject({
      status: 403,
      json: { code: "byos_not_allowed" },
    });
    expect(tableRead.error).not.toBeNull();

    await recordOutcome("team-cloud-03-isolation", {
      expectations: [
        "A non-member cannot bill to, read the settings of, or list the keys of an organization.",
        "A Team-only member cannot save a personal BYOS key: personal BYOS needs personal Pro.",
        "The settings table is not readable directly; members go through get_organization_cloud_settings.",
      ],
      details: {
        outsiderProfile: outsiderProfile.error?.message,
        memberPersonalKey: memberPersonalKey.json,
      },
    });
  } finally {
    await deleteTestUser(admin, outsider.id);
    await cleanup(t);
  }
}, 120000);
