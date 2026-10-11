/**
 * Owners in the organizations Edge Function against the local database:
 * promoting a member, demoting an owner (never the last one), transferring
 * ownership by promote then leave, deleting an organization (refused while
 * it has Team), and what deleting an account through the Auth Admin API does
 * to the organizations the user belonged to.
 *
 * Runs organizations/lib.ts in this process with a service-role RPC, so the
 * rules are the real SQL of 029_organization_owners.sql. Reads go through
 * signed-in clients, so RLS is the real one. Team comes from the billing
 * write functions (../billing.ts).
 */
import type { SupabaseClient } from "@supabase/supabase-js";
import { expect, it } from "vitest";
import { userHasPro } from "../../../supabase/functions/_shared/billing/entitlement";
import { handleOrganizationsRequest } from "../../../supabase/functions/organizations/lib";
import { clearBilling, endPro, grantTeam } from "../billing";
import { getServiceClient } from "../clients";
import { recordOutcome } from "../record";
import {
  createTestUser,
  deleteTestUser,
  signInWithEmailPassword,
  type TestUser,
} from "../seed";

function organizations(admin: SupabaseClient, user: TestUser) {
  return (body: Record<string, unknown>) =>
    handleOrganizationsRequest(body, {
      userId: user.user.id,
      rpc: async (fn, args) => {
        const { data, error } = await admin.rpc(fn, args);
        return { data, error };
      },
      webUrl: "http://localhost:3001",
    });
}

/** Creates an organization owned by `owner` and has each member join it, in order. */
async function organizationWith(
  admin: SupabaseClient,
  name: string,
  owner: TestUser,
  members: TestUser[],
): Promise<string> {
  const asOwner = organizations(admin, owner);
  const created = await asOwner({ action: "create", name });
  expect(created.status).toBe(200);
  const orgId = (created.body.organization as { id: string }).id;
  for (const member of members) {
    const invited = await asOwner({
      action: "invite",
      organization_id: orgId,
      email: member.email,
    });
    const token = new URLSearchParams(
      new URL(String(invited.body.accept_url)).hash.slice(1),
    ).get("invite");
    const joined = await organizations(
      admin,
      member,
    )({ action: "accept", token });
    expect(joined.status).toBe(200);
  }
  return orgId;
}

async function roles(admin: SupabaseClient, orgId: string) {
  const { data } = await admin
    .from("organization_members")
    .select("user_id, role")
    .eq("org_id", orgId)
    .order("created_at");
  return data ?? [];
}

it("lets owners promote, demote, hand over and delete an organization", async () => {
  const admin = getServiceClient();
  const owner = await createTestUser();
  const member = await createTestUser();
  const installationId = Math.floor(Math.random() * 1_000_000_000) + 6_000_000;
  let orgId: string | null = null;

  try {
    orgId = await organizationWith(admin, "Owners QA", owner, [member]);
    const asOwner = organizations(admin, owner);
    const asMember = organizations(admin, member);

    // ── Promote and demote ───────────────────────────────────────────────
    const selfPromote = await asMember({
      action: "promote_member",
      organization_id: orgId,
      user_id: member.user.id,
    });
    expect(selfPromote).toMatchObject({
      status: 403,
      body: { code: "owner_required" },
    });
    const lastOwnerStepsDown = await asOwner({
      action: "demote_owner",
      organization_id: orgId,
      user_id: owner.user.id,
    });
    expect(lastOwnerStepsDown).toMatchObject({
      status: 409,
      body: { code: "last_owner" },
    });

    const promoted = await asOwner({
      action: "promote_member",
      organization_id: orgId,
      user_id: member.user.id,
    });
    expect(promoted).toEqual({
      status: 200,
      body: { ok: true, result: "promoted" },
    });
    const { client: memberClient } = await signInWithEmailPassword(
      member.email,
      member.password,
    );
    const memberSees = await memberClient
      .from("organization_members")
      .select("user_id, role")
      .eq("org_id", orgId)
      .order("created_at");
    expect(memberSees.data).toEqual([
      { user_id: owner.user.id, role: "owner" },
      { user_id: member.user.id, role: "owner" },
    ]);
    const demoted = await asMember({
      action: "demote_owner",
      organization_id: orgId,
      user_id: owner.user.id,
    });
    expect(demoted).toEqual({
      status: 200,
      body: { ok: true, result: "demoted" },
    });
    expect(await roles(admin, orgId)).toEqual([
      { user_id: owner.user.id, role: "member" },
      { user_id: member.user.id, role: "owner" },
    ]);

    await recordOutcome("organization-owners-01-promote-demote", {
      expectations: [
        "A member cannot promote themselves (403 owner_required) and the only owner cannot step down (409 last_owner).",
        "An owner promotes a member (promoted), and the member reads both owners through RLS.",
        "The new owner then demotes the original owner to member (demoted).",
      ],
      details: { selfPromote, lastOwnerStepsDown, promoted, demoted },
    });

    // ── Transfer: promote, then leave ────────────────────────────────────
    await asMember({
      action: "promote_member",
      organization_id: orgId,
      user_id: owner.user.id,
    });
    const left = await asMember({ action: "leave", organization_id: orgId });
    expect(left).toEqual({ status: 200, body: { ok: true } });
    expect(await roles(admin, orgId)).toEqual([
      { user_id: owner.user.id, role: "owner" },
    ]);

    await recordOutcome("organization-owners-02-transfer", {
      expectations: [
        "After promoting another member, an owner can leave (200), which transfers the organization to them.",
      ],
      details: { left, roles: await roles(admin, orgId) },
    });

    // ── Delete: refused with Team, then allowed ──────────────────────────
    await admin.from("github_app_installations").insert({
      id: installationId,
      account_login: `sqa-owners-${installationId}`,
      account_type: "Organization",
      app_id: 1,
      linked_user_id: member.user.id,
      organization_id: orgId,
    });
    const team = await grantTeam(admin, orgId);
    const refused = await asOwner({
      action: "delete_organization",
      organization_id: orgId,
    });
    expect(refused).toEqual({
      status: 409,
      body: {
        error:
          "This organization has a Team subscription. Cancel it with Manage billing, then delete the organization once the subscription has ended.",
        code: "team_subscription_active",
      },
    });
    expect(await userHasPro(admin, owner.user.id)).toBe(true);

    await endPro(admin, team);
    const deleted = await asOwner({
      action: "delete_organization",
      organization_id: orgId,
    });
    expect(deleted).toEqual({ status: 200, body: { ok: true } });
    const gone = await admin.from("organizations").select("id").eq("id", orgId);
    expect(gone.data).toEqual([]);
    const installation = await admin
      .from("github_app_installations")
      .select("organization_id, linked_user_id")
      .eq("id", installationId)
      .single();
    expect(installation.data).toEqual({
      organization_id: null,
      linked_user_id: owner.user.id,
    });

    await recordOutcome("organization-owners-03-delete", {
      expectations: [
        "Deleting an organization with an active Team answers 409 team_subscription_active and says to cancel with Manage billing first.",
        "Once the Team has ended, the owner deletes it (200) and no organization row is left.",
        "Its installation is detached and linked to the owner who deleted the organization.",
      ],
      details: { refused, deleted, installation: installation.data },
    });
  } finally {
    await admin
      .from("github_app_installations")
      .delete()
      .eq("id", installationId);
    if (orgId) {
      await clearBilling(admin, [orgId]);
      await admin.from("organizations").delete().eq("id", orgId);
    }
    for (const user of [owner, member])
      await deleteTestUser(admin, user.user.id);
  }
}, 120_000);

it("hands an organization to its longest-standing member when the last owner deletes their account", async () => {
  const admin = getServiceClient();
  const owner = await createTestUser();
  const first = await createTestUser();
  const second = await createTestUser();
  const loner = await createTestUser();
  const remaining = new Set([owner, first, second, loner]);
  const orgIds: string[] = [];

  try {
    const orgId = await organizationWith(admin, "Handover QA", owner, [
      first,
      second,
    ]);
    orgIds.push(orgId);
    const lonelyOrgId = await organizationWith(admin, "Lonely QA", loner, []);
    orgIds.push(lonelyOrgId);
    await grantTeam(admin, orgId);

    // Through Auth, the way an account is deleted for real.
    await deleteTestUser(admin, owner.user.id);
    remaining.delete(owner);
    expect(await roles(admin, orgId)).toEqual([
      { user_id: first.user.id, role: "owner" },
      { user_id: second.user.id, role: "member" },
    ]);
    expect(await userHasPro(admin, second.user.id)).toBe(true);
    const newOwnerInvites = await organizations(
      admin,
      first,
    )({
      action: "invite",
      organization_id: orgId,
      email: "after-handover@example.com",
    });
    expect(newOwnerInvites.status).toBe(200);

    await deleteTestUser(admin, loner.user.id);
    remaining.delete(loner);
    const lonely = await admin
      .from("organizations")
      .select("id")
      .eq("id", lonelyOrgId);
    expect(lonely.data).toEqual([]);

    await recordOutcome("organization-owners-04-account-deletion", {
      expectations: [
        "Deleting the only owner's account through the Auth Admin API makes the member who joined first the owner; the later member stays a member.",
        "The remaining members keep Team, and the new owner can invite.",
        "Deleting the account of an organization's only member deletes the organization.",
      ],
      details: {
        roles: await roles(admin, orgId),
        newOwnerInvites: newOwnerInvites.status,
        lonely: lonely.data,
      },
    });
  } finally {
    await clearBilling(admin, orgIds);
    if (orgIds.length > 0)
      await admin.from("organizations").delete().in("id", orgIds);
    for (const user of remaining) await deleteTestUser(admin, user.user.id);
  }
}, 120_000);
