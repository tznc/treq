/**
 * Pro enforcement on the GitHub App install flow
 * (`create-github-install-intent`, `complete-github-installation`) against
 * the local database.
 *
 * Runs the functions' handlers (lib.ts) in this process with their real
 * service-role stores, so has_pro, github_install_intents and
 * github_app_installations are the real ones. Only GitHub is a stub
 * (../github.ts): local runs have no GitHub App. Pro comes from the billing
 * write functions (../billing.ts).
 */
import { expect, it } from "vitest";
import {
  PRO_REQUIRED_MESSAGES,
  userHasPro,
} from "../../../supabase/functions/_shared/billing/entitlement";
import { sha256Hex } from "../../../supabase/functions/_shared/intent-state";
import {
  completeInstallation,
  installationLinkStore,
} from "../../../supabase/functions/complete-github-installation/lib";
import {
  createInstallIntent,
  installIntentStore,
} from "../../../supabase/functions/create-github-install-intent/lib";
import { clearBilling, endPro, grantPro } from "../billing";
import { getServiceClient } from "../clients";
import { stubGitHubInstallationAccess } from "../github";
import { recordOutcome } from "../record";
import { createTestUser, deleteTestUser } from "../seed";

it("requires Pro to start and to complete a GitHub App installation", async () => {
  const admin = getServiceClient();
  const free = await createTestUser();
  const pro = await createTestUser();
  const installationId = Math.floor(Math.random() * 1_000_000_000) + 3_000_000;
  const login = `sqa-org-${installationId}`;

  try {
    const grant = await grantPro(admin, pro.user.id);
    const depsFor = (userId: string) => ({
      userId,
      hasPro: () => userHasPro(admin, userId),
      store: installIntentStore(admin),
    });

    // ── create-github-install-intent ──────────────────────────────────────
    const freeIntent = await createInstallIntent(depsFor(free.user.id));
    expect(freeIntent).toEqual({
      status: 402,
      body: { error: PRO_REQUIRED_MESSAGES.githubApp, code: "pro_required" },
    });
    const freeRows = await admin
      .from("github_install_intents")
      .select("id")
      .eq("user_id", free.user.id);
    expect(freeRows.data).toEqual([]);

    const proIntent = await createInstallIntent(depsFor(pro.user.id));
    expect(proIntent.status).toBe(200);
    const state = proIntent.body.state as string;
    expect(state).toMatch(/^[0-9a-f]{64}$/);
    const proRows = await admin
      .from("github_install_intents")
      .select("state_hash, consumed_at")
      .eq("user_id", pro.user.id);
    expect(proRows.data).toEqual([
      { state_hash: await sha256Hex(state), consumed_at: null },
    ]);

    await recordOutcome("billing-enforcement-github-01-intent", {
      expectations: [
        "create-github-install-intent answers a Free user 402 with code pro_required and writes no intent row.",
        "A Pro user gets a 64-hex state and exactly one unconsumed intent row holding its SHA-256 hash.",
      ],
      details: { freeIntent, proStatus: proIntent.status, proRows: proRows.data },
    });

    // ── complete-github-installation ──────────────────────────────────────
    // An intent the Free user holds (say, created while they had Pro) is
    // still refused, and left unconsumed.
    const freeState = "f".repeat(64);
    await admin.from("github_install_intents").insert({
      user_id: free.user.id,
      state_hash: await sha256Hex(freeState),
      expires_at: new Date(Date.now() + 10 * 60_000).toISOString(),
    });
    const linkDeps = (userId: string) => ({
      userId,
      hasPro: () => userHasPro(admin, userId),
      store: installationLinkStore(admin),
      // Both users authorize as a GitHub user who manages the installation.
      github: stubGitHubInstallationAccess(
        { "free-code": [installationId], "pro-code": [installationId] },
        () => ({ login, type: "Organization", avatar_url: null }),
      ),
    });
    const freeComplete = await completeInstallation(
      { installation_id: installationId, state: freeState, code: "free-code" },
      linkDeps(free.user.id),
    );
    expect(freeComplete).toEqual({
      status: 402,
      body: { error: PRO_REQUIRED_MESSAGES.githubApp, code: "pro_required" },
    });
    const freeIntentAfter = await admin
      .from("github_install_intents")
      .select("consumed_at")
      .eq("user_id", free.user.id)
      .single();
    expect(freeIntentAfter.data?.consumed_at).toBeNull();
    const notLinked = await admin
      .from("github_app_installations")
      .select("id")
      .eq("id", installationId);
    expect(notLinked.data).toEqual([]);

    const proComplete = await completeInstallation(
      { installation_id: installationId, state, code: "pro-code" },
      linkDeps(pro.user.id),
    );
    expect(proComplete).toEqual({
      status: 200,
      body: {
        ok: true,
        account_login: login,
        account_type: "Organization",
        organization_id: null,
      },
    });
    const linked = await admin
      .from("github_app_installations")
      .select("linked_user_id, account_login")
      .eq("id", installationId)
      .single();
    expect(linked.data).toEqual({
      linked_user_id: pro.user.id,
      account_login: login,
    });

    await recordOutcome("billing-enforcement-github-02-complete", {
      expectations: [
        "complete-github-installation answers a Free user 402 pro_required, leaves their intent unconsumed and links nothing.",
        "The Pro user's intent completes with 200 and links the installation to them.",
      ],
      details: { freeComplete, proComplete, linked: linked.data },
    });

    // ── Entitlement ends ──────────────────────────────────────────────────
    await endPro(admin, grant);
    const stillLinked = await admin
      .from("github_app_installations")
      .select("linked_user_id")
      .eq("id", installationId)
      .single();
    expect(stillLinked.data?.linked_user_id).toBe(pro.user.id);
    const lapsedIntent = await createInstallIntent(depsFor(pro.user.id));
    expect(lapsedIntent.status).toBe(402);

    await recordOutcome("billing-enforcement-github-03-lapse", {
      expectations: [
        "After the subscription is deleted, the installation stays linked to the user.",
        "The lapsed user can no longer start a new installation (402 pro_required).",
      ],
      details: { stillLinked: stillLinked.data, lapsedIntent },
    });
  } finally {
    await admin.from("github_app_installations").delete().eq("id", installationId);
    await clearBilling(admin, [free.user.id, pro.user.id]);
    await deleteTestUser(admin, free.user.id);
    await deleteTestUser(admin, pro.user.id);
  }
}, 120_000);
