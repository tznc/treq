/**
 * delete-account against the local database and Auth (TREQ-274).
 *
 * Runs the function's handler (lib.ts) in this process with the real
 * service-role store, so the lookups, the remote_instances update, the
 * installation delete, auth.admin.deleteUser, the foreign key cascades and
 * the organizations_on_account_delete trigger are all the real ones. The
 * bearer token is checked by the real Auth server. Only the outbound
 * adapters are stubs: Stripe, the Sprites provider and Linear's revoke
 * endpoint, none of which exist locally.
 */
import type { SupabaseClient } from "@supabase/supabase-js";
import { randomBytes } from "node:crypto";
import { expect, it } from "vitest";
import {
  StripeApiError,
  type StripeClient,
} from "../../../supabase/functions/_shared/billing/stripe-api";
import type { ManagedComputeProvider } from "../../../supabase/functions/_shared/remote/sprites-adapter";
import {
  deleteAccount,
  deleteAccountStoreFor,
} from "../../../supabase/functions/delete-account/lib";
import { handleOrganizationsRequest } from "../../../supabase/functions/organizations/lib";
import { clearBilling, endPro, grantPro, grantTeam } from "../billing";
import { getAnonClient, getServiceClient } from "../clients";
import { recordOutcome } from "../record";
import {
  createTestUser,
  deleteTestUser,
  linkGithubRepo,
  signInWithEmailPassword,
  type TestUser,
} from "../seed";

type Stubs = {
  stripeCalls: string[];
  machinesDeleted: string[];
  linearRevoked: string[];
};

function stubStripe(calls: string[], failDelete = false): StripeClient {
  const canceled = new Set<string>();
  return {
    async request<T>(method: string, path: string): Promise<T> {
      calls.push(`${method} ${path}`);
      const id = path.replace("/v1/subscriptions/", "");
      if (method === "GET") {
        return { id, status: canceled.has(id) ? "canceled" : "active" } as T;
      }
      if (method === "DELETE") {
        if (failDelete) throw new StripeApiError(500, "stubbed outage");
        canceled.add(id);
        return { id, status: "canceled" } as T;
      }
      throw new Error(`unexpected Stripe call ${method} ${path}`);
    },
  };
}

function callDeleteAccount(
  admin: SupabaseClient,
  authorization: string | null,
  stubs: Stubs,
  opts: { body?: unknown; stripeFails?: boolean } = {},
) {
  return deleteAccount({
    method: "POST",
    authorization,
    body: opts.body,
    verifyUser: async (token) => {
      const { data, error } = await getAnonClient().auth.getUser(token);
      return error || !data.user ? null : { id: data.user.id };
    },
    storeFor: (userId) => deleteAccountStoreFor(admin, userId),
    stripe: stubStripe(stubs.stripeCalls, opts.stripeFails),
    provider: () =>
      ({
        async deleteInstance(providerId: string) {
          stubs.machinesDeleted.push(providerId);
        },
      }) as unknown as ManagedComputeProvider,
    revokeLinearToken: async (token) => {
      stubs.linearRevoked.push(token);
    },
  });
}

function newStubs(): Stubs {
  return { stripeCalls: [], machinesDeleted: [], linearRevoked: [] };
}

async function accessToken(user: TestUser): Promise<string> {
  const { client } = await signInWithEmailPassword(user.email, user.password);
  const { data } = await client.auth.getSession();
  return data.session!.access_token;
}

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

async function rowCount(
  admin: SupabaseClient,
  table: string,
  column: string,
  value: string | number,
): Promise<number> {
  const { count, error } = await admin
    .from(table)
    .select("*", { count: "exact", head: true })
    .eq(column, value);
  if (error) throw new Error(`${table}: ${error.message}`);
  return count ?? 0;
}

async function userExists(admin: SupabaseClient, userId: string) {
  const { data } = await admin.auth.admin.getUserById(userId);
  return Boolean(data.user);
}

it("rejects callers without a valid token and changes nothing", async () => {
  const admin = getServiceClient();
  const target = await createTestUser();
  try {
    const stubs = newStubs();
    const missing = await callDeleteAccount(admin, null, stubs);
    const forged = await callDeleteAccount(admin, "Bearer not-a-jwt", stubs);

    expect(missing.status).toBe(401);
    expect(forged.status).toBe(401);
    expect(await userExists(admin, target.user.id)).toBe(true);
    expect(stubs).toEqual(newStubs());

    await recordOutcome("delete-account-01-unauthenticated", {
      expectations: [
        "A request with no Authorization header answers 401.",
        "A request whose bearer token Auth does not accept answers 401.",
        "Neither request deletes any user or calls Stripe, the provider or Linear.",
      ],
      details: { missing, forged },
    });
  } finally {
    await deleteTestUser(admin, target.user.id).catch(() => {});
  }
}, 60000);

it("deletes only the caller and releases their billing, machines and integrations", async () => {
  const admin = getServiceClient();
  const owner = await createTestUser();
  const bystander = await createTestUser();
  const teammate = await createTestUser();
  const orgIds: string[] = [];

  try {
    const ownPro = await grantPro(admin, owner.user.id);
    const bystanderPro = await grantPro(admin, bystander.user.id);

    // An organization only the owner belongs to, and one shared with a
    // teammate. Both have Team.
    const soloOrg = await organizationWith(admin, "Solo QA", owner, []);
    const sharedOrg = await organizationWith(admin, "Shared QA", owner, [
      teammate,
    ]);
    orgIds.push(soloOrg, sharedOrg);
    const soloTeam = await grantTeam(admin, soloOrg);
    const sharedTeam = await grantTeam(admin, sharedOrg);

    const providerId = `dev-treq-sqa-${randomBytes(4).toString("hex")}`;
    const { error: instanceError } = await admin
      .from("remote_instances")
      .insert({
        owner_user_id: owner.user.id,
        provider_kind: "fly_sprites",
        provider_resource_id: providerId,
        region: "us_east",
        size_preset: "small",
        status: "ready",
      });
    expect(instanceError).toBeNull();

    const linearToken = `lin_oauth_sqa_${randomBytes(6).toString("hex")}`;
    const { error: linearError } = await admin
      .from("linear_oauth_tokens")
      .insert({
        user_id: owner.user.id,
        access_token: linearToken,
        linear_workspace_id: "ws_sqa",
        linear_workspace_name: "Service QA",
      });
    expect(linearError).toBeNull();

    const personal = await linkGithubRepo(admin, owner.user.id);
    const bystanderRepo = await linkGithubRepo(admin, bystander.user.id);

    const ownerToken = `Bearer ${await accessToken(owner)}`;

    // The owner is the last owner of both organizations, the shared one and
    // the one only they belong to, so deletion is refused before anything
    // is released.
    const blockedStubs = newStubs();
    const blocked = await callDeleteAccount(admin, ownerToken, blockedStubs);
    expect(blocked).toEqual({
      status: 409,
      body: {
        error:
          "You are the last owner of Shared QA, Solo QA. Make another member an owner, or delete the organization, before deleting your account.",
        code: "last_owner",
        organizations: [
          { id: sharedOrg, name: "Shared QA" },
          { id: soloOrg, name: "Solo QA" },
        ],
      },
    });
    expect(blockedStubs).toEqual(newStubs());
    expect(await userExists(admin, owner.user.id)).toBe(true);
    expect(
      await rowCount(admin, "remote_instances", "owner_user_id", owner.user.id),
    ).toBe(1);
    expect(await rowCount(admin, "organizations", "id", soloOrg)).toBe(1);

    await recordOutcome("delete-account-02-last-owner-blocked", {
      expectations: [
        "While the user is the last owner of an organization, delete-account answers 409 last_owner naming every such organization.",
        "That includes the organization the user is the only member of, not just the shared one.",
        "Nothing is released: no Stripe, provider or Linear call, and the user, instance and organizations remain.",
      ],
      details: { blocked, stubs: blockedStubs },
    });

    const promoted = await organizations(
      admin,
      owner,
    )({
      action: "promote_member",
      organization_id: sharedOrg,
      user_id: teammate.user.id,
    });
    expect(promoted.status).toBe(200);

    // Promoting the teammate settles the shared organization; the solo one
    // still blocks until it is deleted.
    const stillBlocked = await callDeleteAccount(admin, ownerToken, newStubs());
    expect(stillBlocked.status).toBe(409);
    expect(stillBlocked.body.organizations).toEqual([
      { id: soloOrg, name: "Solo QA" },
    ]);
    // organization_delete refuses while Team is live, so end it first, as
    // the Stripe portal cancellation would.
    await endPro(admin, soloTeam);
    const removed = await organizations(
      admin,
      owner,
    )({ action: "delete_organization", organization_id: soloOrg });
    expect(removed.status).toBe(200);
    expect(await rowCount(admin, "organizations", "id", soloOrg)).toBe(0);

    // The caller names another user in the body. It must be ignored.
    const stubs = newStubs();
    const result = await callDeleteAccount(admin, ownerToken, stubs, {
      body: { user_id: bystander.user.id },
    });
    expect(result).toEqual({ status: 200, body: { deleted: true } });

    expect(await userExists(admin, owner.user.id)).toBe(false);
    expect(await userExists(admin, bystander.user.id)).toBe(true);
    expect(await rowCount(admin, "profiles", "id", bystander.user.id)).toBe(1);
    expect(
      await rowCount(
        admin,
        "github_app_installations",
        "id",
        bystanderRepo.installationId,
      ),
    ).toBe(1);
    expect(stubs.stripeCalls).not.toContain(
      `DELETE /v1/subscriptions/${bystanderPro.subscriptionId}`,
    );

    await recordOutcome("delete-account-03-caller-only", {
      expectations: [
        "With a body naming another user, delete-account answers 200 and deletes the token's user only.",
        "The other user, their profile, their GitHub installation and their subscription are untouched.",
      ],
      details: {
        result,
        ownerId: owner.user.id,
        bystanderId: bystander.user.id,
      },
    });

    expect(
      stubs.stripeCalls.filter((c) => c.startsWith("DELETE")).sort(),
    ).toEqual([`DELETE /v1/subscriptions/${ownPro.subscriptionId}`]);
    expect(stubs.stripeCalls).not.toContain(
      `DELETE /v1/subscriptions/${sharedTeam.subscriptionId}`,
    );
    expect(stubs.machinesDeleted).toEqual([providerId]);
    expect(stubs.linearRevoked).toEqual([linearToken]);

    await recordOutcome("delete-account-04-external-cleanup", {
      expectations: [
        "Stripe cancels only the user's own Pro, not the shared organization's Team.",
        "The user's cloud workspace machine is deleted at the provider.",
        "The user's Linear access token is revoked.",
      ],
      details: stubs,
    });

    for (const [table, column] of [
      ["profiles", "id"],
      ["remote_instances", "owner_user_id"],
      ["linear_oauth_tokens", "user_id"],
      ["organization_members", "user_id"],
    ] as const) {
      expect(await rowCount(admin, table, column, owner.user.id)).toBe(0);
    }
    expect(
      await rowCount(
        admin,
        "github_app_installations",
        "id",
        personal.installationId,
      ),
    ).toBe(0);
    expect(
      await rowCount(admin, "github_repositories", "id", personal.repoId),
    ).toBe(0);
    expect(await rowCount(admin, "organizations", "id", soloOrg)).toBe(0);
    const { data: sharedMembers } = await admin
      .from("organization_members")
      .select("user_id, role")
      .eq("org_id", sharedOrg);
    expect(sharedMembers).toEqual([
      { user_id: teammate.user.id, role: "owner" },
    ]);
    expect(
      await rowCount(
        admin,
        "billing_subscriptions",
        "stripe_subscription_id",
        ownPro.subscriptionId,
      ),
    ).toBe(1);

    await recordOutcome("delete-account-05-rows", {
      expectations: [
        "The user's profile, remote instance, Linear token, memberships and personal GitHub installation (with its repositories) are gone.",
        "The shared organization survives with the teammate (promoted before deletion) as its only owner.",
        "billing_subscriptions keeps the user's subscription row for accounting.",
      ],
      details: { sharedMembers, soloOrg, sharedOrg },
    });
  } finally {
    for (const user of [owner, bystander, teammate]) {
      await deleteTestUser(admin, user.user.id).catch(() => {});
    }
    for (const orgId of orgIds) {
      await admin.from("organizations").delete().eq("id", orgId);
    }
    await clearBilling(admin, [
      owner.user.id,
      bystander.user.id,
      teammate.user.id,
      ...orgIds,
    ]);
  }
}, 60000);

it("keeps the account when Stripe fails and finishes on retry", async () => {
  const admin = getServiceClient();
  const user = await createTestUser();
  try {
    const pro = await grantPro(admin, user.user.id);
    const token = `Bearer ${await accessToken(user)}`;

    const failing = newStubs();
    const failed = await callDeleteAccount(admin, token, failing, {
      stripeFails: true,
    });
    expect(failed.status).toBe(502);
    expect(await userExists(admin, user.user.id)).toBe(true);
    expect(await rowCount(admin, "profiles", "id", user.user.id)).toBe(1);
    // The user can still sign in after the failed attempt.
    await signInWithEmailPassword(user.email, user.password);

    await recordOutcome("delete-account-06-stripe-failure", {
      expectations: [
        "When Stripe fails to cancel the subscription, delete-account answers 502.",
        "The auth user and profile remain and the user can still sign in.",
      ],
      details: { failed, stripeCalls: failing.stripeCalls },
    });

    const retry = newStubs();
    const done = await callDeleteAccount(admin, token, retry);
    expect(done.status).toBe(200);
    expect(retry.stripeCalls).toContain(
      `DELETE /v1/subscriptions/${pro.subscriptionId}`,
    );
    expect(await userExists(admin, user.user.id)).toBe(false);

    // The deleted user's token no longer authenticates, so a repeat call
    // cannot act on anyone.
    const again = await callDeleteAccount(admin, token, newStubs());
    expect(again.status).toBe(401);
    const { error: signInError } =
      await getAnonClient().auth.signInWithPassword({
        email: user.email,
        password: user.password,
      });
    expect(signInError).not.toBeNull();

    await recordOutcome("delete-account-07-retry", {
      expectations: [
        "Retrying with the same token cancels the subscription and deletes the user (200).",
        "Afterwards the old token answers 401 and password sign-in fails.",
      ],
      details: { done, again, signInError: signInError?.message },
    });
  } finally {
    await deleteTestUser(admin, user.user.id).catch(() => {});
    await clearBilling(admin, [user.user.id]);
  }
}, 60000);
