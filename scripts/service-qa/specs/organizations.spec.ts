/**
 * The organizations Edge Function and Team installation ownership against
 * the local database.
 *
 * Runs the handlers (organizations/lib.ts, complete-github-installation/
 * lib.ts) in this process with a service-role RPC, so every rule runs in the
 * real SQL functions of 028_organizations_team.sql. Reads go through
 * signed-in clients, so RLS is the real one. Only GitHub is a stub
 * (../github.ts): local runs have no GitHub App. Team comes from the billing
 * write functions (../billing.ts).
 */
import type { SupabaseClient } from "@supabase/supabase-js";
import { expect, it } from "vitest";
import { userHasPro } from "../../../supabase/functions/_shared/billing/entitlement";
import {
  completeInstallation,
  installationLinkStore,
} from "../../../supabase/functions/complete-github-installation/lib";
import {
  createInstallIntent,
  installIntentStore,
} from "../../../supabase/functions/create-github-install-intent/lib";
import { handleOrganizationsRequest } from "../../../supabase/functions/organizations/lib";
import { clearBilling, grantPro, grantTeam } from "../billing";
import { getServiceClient } from "../clients";
import { stubGitHubInstallationAccess } from "../github";
import { recordOutcome } from "../record";
import {
  createTestUser,
  deleteTestUser,
  signInWithEmailPassword,
  type TestUser,
} from "../seed";

const WEB_URL = "http://localhost:3001";

function organizations(admin: SupabaseClient, user: TestUser) {
  return (body: Record<string, unknown>) =>
    handleOrganizationsRequest(body, {
      userId: user.user.id,
      rpc: async (fn, args) => {
        const { data, error } = await admin.rpc(fn, args);
        return { data, error };
      },
      webUrl: WEB_URL,
    });
}

function tokenOf(acceptUrl: unknown): string {
  const token = new URLSearchParams(
    new URL(String(acceptUrl)).hash.slice(1),
  ).get("invite");
  if (!token) throw new Error(`no invite token in ${String(acceptUrl)}`);
  return token;
}

it("runs an organization: invites by token, no member cap and Team for every member", async () => {
  const admin = getServiceClient();
  const owner = await createTestUser();
  const member = await createTestUser();
  const outsider = await createTestUser();
  const racers = [await createTestUser(), await createTestUser()];
  const everyone = [owner, member, outsider, ...racers];
  let orgId: string | null = null;

  try {
    const asOwner = organizations(admin, owner);

    // ── Create and invite ────────────────────────────────────────────────
    const created = await asOwner({ action: "create", name: "Service QA Co" });
    expect(created.status).toBe(200);
    orgId = (created.body.organization as { id: string }).id;

    const invited = await asOwner({
      action: "invite",
      organization_id: orgId,
      email: "Teammate@Example.com",
    });
    expect(invited.status).toBe(200);
    expect(invited.body.accept_url).toMatch(
      /^http:\/\/localhost:3001\/dashboard\?tab=team#invite=[0-9a-f]{64}$/,
    );
    expect(invited.body).not.toHaveProperty("token");
    const token = tokenOf(invited.body.accept_url);

    const { client: ownerClient } = await signInWithEmailPassword(
      owner.email,
      owner.password,
    );
    const ownerInvites = await ownerClient
      .from("organization_invites")
      .select("email, accepted_at");
    expect(ownerInvites.data).toEqual([
      { email: "teammate@example.com", accepted_at: null },
    ]);
    const hashRead = await ownerClient
      .from("organization_invites")
      .select("token_hash");
    expect(hashRead.error?.code).toBe("42501");

    // The member signs in with a different address than the invite's.
    const asMember = organizations(admin, member);
    const accepted = await asMember({ action: "accept", token });
    expect(accepted).toEqual({
      status: 200,
      body: {
        organization: { id: orgId, name: "Service QA Co" },
        result: "joined",
      },
    });
    const replay = await organizations(
      admin,
      outsider,
    )({ action: "accept", token });
    expect(replay.status).toBe(410);

    const { client: memberClient } = await signInWithEmailPassword(
      member.email,
      member.password,
    );
    const memberView = await memberClient
      .from("organization_members")
      .select("user_id, role")
      .order("role", { ascending: false });
    expect(memberView.data).toEqual([
      { user_id: owner.user.id, role: "owner" },
      { user_id: member.user.id, role: "member" },
    ]);
    const memberInvites = await memberClient
      .from("organization_invites")
      .select("id");
    expect(memberInvites.data).toEqual([]);

    const { client: outsiderClient } = await signInWithEmailPassword(
      outsider.email,
      outsider.password,
    );
    const outsiderView = await outsiderClient
      .from("organizations")
      .select("id");
    expect(outsiderView.data).toEqual([]);

    await recordOutcome("organizations-01-invite-by-token", {
      expectations: [
        "invite answers an accept URL on the Team tab and never a bare token; the owner reads the invite's email but gets 42501 on token_hash.",
        "A different account accepts with the token and joins as member; the same token answers 410 to anyone after that.",
        "The member reads the member list but no invites; an outsider reads no organization.",
      ],
      details: { orgId, accepted, replay, memberView: memberView.data },
    });

    // ── No member cap, and two accepts racing for one invite ──────────────────
    const seatInvites = [];
    for (let i = 0; i < 8; i += 1) {
      const seat = await asOwner({
        action: "invite",
        organization_id: orgId,
        email: `seat${i}@example.com`,
      });
      expect(seat.status).toBe(200);
      seatInvites.push(seat);
    }
    const eleventh = await asOwner({
      action: "invite",
      organization_id: orgId,
      email: "eleventh@example.com",
    });
    // Team has no member cap: seats beyond 5 are billed, not refused.
    expect(eleventh.status).toBe(200);

    const raceToken = tokenOf(seatInvites[0].body.accept_url);
    const race = await Promise.all(
      racers.map((racer) =>
        organizations(admin, racer)({ action: "accept", token: raceToken }),
      ),
    );
    expect(race.map((r) => r.status).sort()).toEqual([200, 410]);
    const { count: memberCount } = await admin
      .from("organization_members")
      .select("user_id", { count: "exact", head: true })
      .eq("org_id", orgId);
    expect(memberCount).toBe(3);

    await recordOutcome("organizations-02-seat-cap-and-race", {
      expectations: [
        "With 2 members and 8 pending invites, an eleventh seat is still invited: Team has no member cap.",
        "Two accounts accepting the same token at the same moment: exactly one gets 200, the other 410, and the organization gains one member.",
      ],
      details: {
        eleventh,
        raceStatuses: race.map((r) => r.status),
        memberCount,
      },
    });

    // ── Team gives every member Pro, and removal ends it at once ─────────
    expect(await userHasPro(admin, member.user.id)).toBe(false);
    await grantTeam(admin, orgId);
    expect(await userHasPro(admin, member.user.id)).toBe(true);
    expect(await userHasPro(admin, outsider.user.id)).toBe(false);
    const memberSubscription = await memberClient
      .from("subscriptions")
      .select("plan, status")
      .single();
    expect(memberSubscription.data).toEqual({ plan: "pro", status: "active" });

    const memberRemoves = await asMember({
      action: "remove_member",
      organization_id: orgId,
      user_id: owner.user.id,
    });
    expect(memberRemoves.status).toBe(403);
    const lastOwnerLeaves = await asOwner({
      action: "leave",
      organization_id: orgId,
    });
    expect(lastOwnerLeaves).toMatchObject({
      status: 409,
      body: { code: "last_owner" },
    });

    const removed = await asOwner({
      action: "remove_member",
      organization_id: orgId,
      user_id: member.user.id,
    });
    expect(removed).toEqual({ status: 200, body: { ok: true } });
    expect(await userHasPro(admin, member.user.id)).toBe(false);
    const afterRemoval = await memberClient
      .from("subscriptions")
      .select("plan")
      .single();
    expect(afterRemoval.data).toEqual({ plan: "free" });

    await recordOutcome("organizations-03-team-entitlement", {
      expectations: [
        "Once the organization has an active Team subscription, has_pro is true for its member and public.subscriptions reads plan pro / active for them; an outsider stays Free.",
        "A member cannot remove the owner (403) and the only owner cannot leave (409 last_owner).",
        "Removing the member ends their Pro at once: has_pro false and subscriptions plan free.",
      ],
      details: {
        memberSubscription: memberSubscription.data,
        afterRemoval: afterRemoval.data,
      },
    });
  } finally {
    if (orgId) {
      await clearBilling(admin, [orgId]);
      await admin.from("organizations").delete().eq("id", orgId);
    }
    for (const user of everyone) await deleteTestUser(admin, user.user.id);
  }
}, 120_000);

it("lets only an organization's owners relink its GitHub App installation", async () => {
  const admin = getServiceClient();
  const owner = await createTestUser();
  const member = await createTestUser();
  const outsider = await createTestUser();
  const installationId = Math.floor(Math.random() * 1_000_000_000) + 4_000_000;
  const login = `sqa-team-${installationId}`;
  let orgId: string | null = null;

  const linkDeps = (userId: string) => ({
    userId,
    hasPro: () => userHasPro(admin, userId),
    store: installationLinkStore(admin),
    // Everyone here authorizes as a GitHub user who can access the
    // installation, so only the Treq organization rule decides.
    github: stubGitHubInstallationAccess(
      { "github-admin": [installationId] },
      () => ({
        login,
        type: "Organization",
        avatar_url: null,
      }),
    ),
  });
  async function linkAs(user: TestUser) {
    const intent = await createInstallIntent({
      userId: user.user.id,
      hasPro: () => userHasPro(admin, user.user.id),
      store: installIntentStore(admin),
    });
    expect(intent.status).toBe(200);
    return completeInstallation(
      {
        installation_id: installationId,
        state: intent.body.state,
        code: "github-admin",
      },
      linkDeps(user.user.id),
    );
  }

  try {
    const asOwner = organizations(admin, owner);
    orgId = (
      (await asOwner({ action: "create", name: "Relink Co" })).body
        .organization as { id: string }
    ).id;
    const invited = await asOwner({
      action: "invite",
      organization_id: orgId,
      email: "relink-member@example.com",
    });
    await organizations(
      admin,
      member,
    )({
      action: "accept",
      token: tokenOf(invited.body.accept_url),
    });
    await grantTeam(admin, orgId);
    await grantPro(admin, outsider.user.id);

    // The owner links a GitHub organization's installation, then attaches it.
    const linked = await linkAs(owner);
    expect(linked).toEqual({
      status: 200,
      body: {
        ok: true,
        account_login: login,
        account_type: "Organization",
        organization_id: null,
      },
    });
    const notYours = await organizations(
      admin,
      member,
    )({
      action: "attach_installation",
      organization_id: orgId,
      installation_id: installationId,
    });
    expect(notYours.status).toBe(403);
    const attached = await asOwner({
      action: "attach_installation",
      organization_id: orgId,
      installation_id: installationId,
    });
    expect(attached).toEqual({
      status: 200,
      body: { ok: true, result: "attached" },
    });

    await recordOutcome("organizations-04-attach", {
      expectations: [
        "Completing the install of a GitHub Organization account answers account_type Organization and organization_id null, so the dashboard can ask which Treq organization owns it.",
        "A member cannot attach it (403); the owner who linked it attaches it (attached).",
      ],
      details: { linked, notYours, attached },
    });

    // ── Nobody but an owner can take it over now ─────────────────────────
    const outsiderRelink = await linkAs(outsider);
    expect(outsiderRelink).toMatchObject({
      status: 403,
      body: { code: "organization_owner_required" },
    });
    const memberRelink = await linkAs(member);
    expect(memberRelink.status).toBe(403);
    const row = await admin
      .from("github_app_installations")
      .select("linked_user_id, organization_id")
      .eq("id", installationId)
      .single();
    expect(row.data).toEqual({
      linked_user_id: owner.user.id,
      organization_id: orgId,
    });
    const ownerRelink = await linkAs(owner);
    expect(ownerRelink).toMatchObject({
      status: 200,
      body: { organization_id: orgId },
    });

    const { client: memberClient } = await signInWithEmailPassword(
      member.email,
      member.password,
    );
    const memberSees = await memberClient
      .from("github_app_installations")
      .select("id, organization_id");
    expect(memberSees.data).toEqual([
      { id: installationId, organization_id: orgId },
    ]);

    await recordOutcome("organizations-05-relink-protection", {
      expectations: [
        "After the attach, an outsider with Pro and a member with Team Pro both get 403 organization_owner_required from complete-github-installation, and linked_user_id stays the owner's.",
        "The owner can still relink, and the answer names the organization.",
        "The member reads the organization's installation through RLS.",
      ],
      details: {
        outsiderRelink,
        memberRelink,
        row: row.data,
        memberSees: memberSees.data,
      },
    });
  } finally {
    await admin
      .from("github_app_installations")
      .delete()
      .eq("id", installationId);
    if (orgId) {
      await clearBilling(admin, [orgId, outsider.user.id]);
      await admin.from("organizations").delete().eq("id", orgId);
    }
    for (const user of [owner, member, outsider]) {
      await deleteTestUser(admin, user.user.id);
    }
  }
}, 120_000);
