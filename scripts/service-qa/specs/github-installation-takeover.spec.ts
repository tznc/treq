/**
 * A forged GitHub setup redirect cannot take over someone else's GitHub App
 * installation (complete-github-installation).
 *
 * GitHub's setup redirect carries an installation_id anyone can edit. The
 * handler (lib.ts) runs in this process against the local database with its
 * real intent store, has_pro and github_link_installation. GitHub is the
 * stub in ../github.ts: each OAuth code is one GitHub user and the
 * installations that user can access.
 */
import { expect, it } from "vitest";
import { userHasPro } from "../../../supabase/functions/_shared/billing/entitlement";
import { sha256Hex } from "../../../supabase/functions/_shared/intent-state";
import {
  completeInstallation,
  installationLinkStore,
} from "../../../supabase/functions/complete-github-installation/lib";
import {
  createInstallIntent,
  installIntentStore,
} from "../../../supabase/functions/create-github-install-intent/lib";
import { handleOrganizationsRequest } from "../../../supabase/functions/organizations/lib";
import { clearBilling, grantPro } from "../billing";
import { getServiceClient } from "../clients";
import { stubGitHubInstallationAccess } from "../github";
import { recordOutcome } from "../record";
import { createTestUser, deleteTestUser, type TestUser } from "../seed";

it("refuses to link an installation the GitHub user cannot access", async () => {
  const admin = getServiceClient();
  const victim = await createTestUser();
  const attacker = await createTestUser();
  const base = Math.floor(Math.random() * 1_000_000_000) + 5_000_000;
  const victimInstallation = base;
  const attackerInstallation = base + 1;
  const unlinkedInstallation = base + 2;
  const installationIds = [
    victimInstallation,
    attackerInstallation,
    unlinkedInstallation,
  ];
  let attackerOrgId: string | null = null;

  // victim-gh manages the victim's GitHub organization and the never-linked
  // one; attacker-gh only manages the attacker's own.
  const github = stubGitHubInstallationAccess(
    {
      "victim-gh": [victimInstallation, unlinkedInstallation],
      "attacker-gh": [attackerInstallation],
    },
    (id) => ({
      login: `sqa-takeover-${id}`,
      type: "Organization",
      avatar_url: null,
    }),
  );
  const deps = (user: TestUser) => ({
    userId: user.user.id,
    hasPro: () => userHasPro(admin, user.user.id),
    store: installationLinkStore(admin),
    github,
  });
  async function intentFor(user: TestUser): Promise<string> {
    const intent = await createInstallIntent({
      userId: user.user.id,
      hasPro: () => userHasPro(admin, user.user.id),
      store: installIntentStore(admin),
    });
    expect(intent.status).toBe(200);
    return intent.body.state as string;
  }
  const installationRow = (id: number) =>
    admin
      .from("github_app_installations")
      .select("linked_user_id, organization_id")
      .eq("id", id)
      .maybeSingle();

  try {
    await grantPro(admin, victim.user.id);
    await grantPro(admin, attacker.user.id);

    const victimLinked = await completeInstallation(
      {
        installation_id: victimInstallation,
        state: await intentFor(victim),
        code: "victim-gh",
      },
      deps(victim),
    );
    expect(victimLinked.status).toBe(200);

    // ── The attacker edits installation_id on their own redirect ──────────
    const attackerState = await intentFor(attacker);
    const takeover = await completeInstallation(
      {
        installation_id: victimInstallation,
        state: attackerState,
        code: "attacker-gh",
      },
      deps(attacker),
    );
    expect(takeover).toEqual({
      status: 403,
      body: {
        error: expect.stringContaining(
          "cannot access this GitHub App installation",
        ),
        code: "installation_not_accessible",
      },
    });
    expect((await installationRow(victimInstallation)).data).toEqual({
      linked_user_id: victim.user.id,
      organization_id: null,
    });
    const spent = await admin
      .from("github_install_intents")
      .select("consumed_at")
      .eq("state_hash", await sha256Hex(attackerState))
      .single();
    expect(spent.data?.consumed_at).not.toBeNull();
    const retry = await completeInstallation(
      {
        installation_id: victimInstallation,
        state: attackerState,
        code: "attacker-gh",
      },
      deps(attacker),
    );
    expect(retry).toEqual({
      status: 403,
      body: { error: "Install intent is invalid, expired or already used" },
    });

    await recordOutcome("github-installation-takeover-01-forged-id", {
      expectations: [
        "An attacker with Pro and a valid intent who sends the victim's installation_id with their own GitHub authorization gets 403 installation_not_accessible.",
        "The victim's installation stays linked to the victim, with no organization.",
        "The attacker's intent is consumed, so the forged callback cannot be replayed.",
      ],
      details: { takeover, retry, spent: spent.data },
    });

    // ── Without GitHub's code, or for an installation nobody linked yet ──
    const noCodeState = await intentFor(attacker);
    const noCode = await completeInstallation(
      { installation_id: victimInstallation, state: noCodeState },
      deps(attacker),
    );
    expect(noCode).toMatchObject({
      status: 400,
      body: { code: "github_authorization_required" },
    });
    const untouched = await admin
      .from("github_install_intents")
      .select("consumed_at")
      .eq("state_hash", await sha256Hex(noCodeState))
      .single();
    expect(untouched.data?.consumed_at).toBeNull();

    const unlinked = await completeInstallation(
      {
        installation_id: unlinkedInstallation,
        state: noCodeState,
        code: "attacker-gh",
      },
      deps(attacker),
    );
    expect(unlinked).toMatchObject({
      status: 403,
      body: { code: "installation_not_accessible" },
    });
    expect((await installationRow(unlinkedInstallation)).data).toBeNull();

    await recordOutcome(
      "github-installation-takeover-02-no-code-and-unlinked",
      {
        expectations: [
          "Without a code the handler answers 400 github_authorization_required before touching the intent, so a misconfigured App fails closed.",
          "An installation nobody has linked yet is refused the same way (403) when the attacker's GitHub user cannot access it, and no row is created.",
        ],
        details: { noCode, unlinked },
      },
    );

    // ── Nor can the attacker attach it to their own organization ─────────
    const asAttacker = (body: Record<string, unknown>) =>
      handleOrganizationsRequest(body, {
        userId: attacker.user.id,
        rpc: async (fn, args) => {
          const { data, error } = await admin.rpc(fn, args);
          return { data, error };
        },
        webUrl: "http://localhost:3001",
      });
    const created = await asAttacker({ action: "create", name: "Takeover Co" });
    attackerOrgId = (created.body.organization as { id: string }).id;
    const attach = await asAttacker({
      action: "attach_installation",
      organization_id: attackerOrgId,
      installation_id: victimInstallation,
    });
    expect(attach).toMatchObject({
      status: 404,
      body: { code: "installation_not_found" },
    });

    const ownLinked = await completeInstallation(
      {
        installation_id: attackerInstallation,
        state: await intentFor(attacker),
        code: "attacker-gh",
      },
      deps(attacker),
    );
    expect(ownLinked.status).toBe(200);
    expect((await installationRow(attackerInstallation)).data).toEqual({
      linked_user_id: attacker.user.id,
      organization_id: null,
    });

    await recordOutcome("github-installation-takeover-03-attach-and-own", {
      expectations: [
        "The attacker cannot attach the victim's installation to their own organization (404 installation_not_found).",
        "The same attacker still links the installation their GitHub user does manage (200).",
      ],
      details: { attach, ownLinked },
    });
  } finally {
    await admin
      .from("github_app_installations")
      .delete()
      .in("id", installationIds);
    if (attackerOrgId)
      await admin.from("organizations").delete().eq("id", attackerOrgId);
    await clearBilling(admin, [victim.user.id, attacker.user.id]);
    await deleteTestUser(admin, victim.user.id);
    await deleteTestUser(admin, attacker.user.id);
  }
}, 120_000);
