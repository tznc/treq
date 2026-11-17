// Applies verified GitHub webhook payloads: upserts the directly observable
// GitHub facts (installation and repository rows) with fast, idempotent DB
// writes.

import type { SupabaseClient } from "npm:@supabase/supabase-js@2.95.3";

// ── Payload shapes ───────────────────────────────────────────────────────────

// Only the fields this module reads; GitHub sends far more.
export interface GitHubRepositoryPayload {
  id: number;
  name: string;
  full_name: string;
  private?: boolean;
  default_branch?: string;
}

export interface GitHubWebhookPayload {
  action?: string;
  installation?: {
    id: number;
    app_id: number;
    account: { login: string; type: string; avatar_url?: string | null };
  };
  repositories?: GitHubRepositoryPayload[];
  repositories_added?: GitHubRepositoryPayload[];
  repositories_removed?: { id: number }[];
  repository?: { id: number };
}

// ── Directly observable GitHub facts ─────────────────────────────────────────

export async function applyInstallationFacts(
  supabase: SupabaseClient,
  event: string,
  payload: GitHubWebhookPayload,
): Promise<void> {
  const installation = payload.installation;
  const action = payload.action as string;
  if (!installation) return;

  if (event === "installation") {
    if (action === "deleted") {
      await supabase
        .from("github_app_installations")
        .delete()
        .eq("id", installation.id);
      return;
    }
    if (action === "suspend") {
      await supabase
        .from("github_app_installations")
        .update({
          suspended_at: new Date().toISOString(),
          updated_at: new Date().toISOString(),
        })
        .eq("id", installation.id);
      return;
    }
    if (action === "unsuspend") {
      await supabase
        .from("github_app_installations")
        .update({ suspended_at: null, updated_at: new Date().toISOString() })
        .eq("id", installation.id);
      return;
    }

    // created / new_permissions_accepted. Deliberately NOT setting
    // linked_user_id here: linking to a Treq user happens only through the
    // authenticated installation-intent flow.
    await supabase.from("github_app_installations").upsert(
      {
        id: installation.id,
        account_login: installation.account.login,
        account_type: installation.account.type,
        account_avatar_url: installation.account.avatar_url ?? null,
        app_id: installation.app_id,
        updated_at: new Date().toISOString(),
      },
      { onConflict: "id" },
    );
    await upsertRepos(supabase, installation.id, payload.repositories ?? []);
  }

  if (event === "installation_repositories") {
    await upsertRepos(supabase, installation.id, payload.repositories_added ?? []);
    const removed = (payload.repositories_removed ?? []).map((r) => r.id);
    if (removed.length > 0) {
      const { error } = await supabase.from("github_repositories").delete().in("id", removed);
      if (error) throw new Error(`removing repositories: ${error.message}`);
    }
  }
}

async function upsertRepos(
  supabase: SupabaseClient,
  installationId: number,
  repos: GitHubRepositoryPayload[],
): Promise<void> {
  if (repos.length === 0) return;
  await supabase.from("github_repositories").upsert(
    repos.map((r) => ({
      id: r.id,
      installation_id: installationId,
      owner: r.full_name.split("/")[0],
      name: r.name,
      full_name: r.full_name,
      private: r.private ?? false,
      default_branch: r.default_branch ?? "main",
      updated_at: new Date().toISOString(),
    })),
    { onConflict: "id" },
  );
}
