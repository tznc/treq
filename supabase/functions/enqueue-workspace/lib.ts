// Access check for enqueue-workspace, free of Deno imports so it runs under
// service-qa. Adding a PR to the merge queue labels it through the GitHub
// App, so it needs a linked installation whose owner has Pro
// (prds/billing-and-teams.md, "Enforcement"). Removing a PR from the queue
// always works, so a user whose Pro ended can still clear their queue.

import {
  installationHasPro,
  PRO_REQUIRED_MESSAGES,
  proRequired,
  type RpcClient,
} from "../_shared/billing/entitlement.ts";
import type { ServiceClientLike } from "../_shared/billing/store.ts";
import type { HandlerResult } from "../_shared/intent-state.ts";

export type QueueRepo = {
  id: number;
  owner: string;
  name: string;
  installation_id: number;
};

export interface QueueAccessStore {
  findRepo(fullName: string): Promise<QueueRepo | null>;
  installationOwner(installationId: number): Promise<string | null>;
  installationHasPro(installationId: number): Promise<boolean>;
}

export async function authorizeQueueAction(
  request: { userId: string; repoFullName: string; action: "enqueue" | "dequeue" },
  store: QueueAccessStore,
): Promise<{ ok: true; repo: QueueRepo } | ({ ok: false } & HandlerResult)> {
  const repo = await store.findRepo(request.repoFullName);
  if (!repo) {
    return {
      ok: false,
      status: 404,
      body: { error: "Repository not found or not connected" },
    };
  }
  if ((await store.installationOwner(repo.installation_id)) !== request.userId) {
    return { ok: false, status: 403, body: { error: "Forbidden" } };
  }
  if (
    request.action === "enqueue" &&
    !(await store.installationHasPro(repo.installation_id))
  ) {
    return { ok: false, ...proRequired(PRO_REQUIRED_MESSAGES.mergeQueue) };
  }
  return { ok: true, repo };
}

export function queueAccessStore(
  client: ServiceClientLike & RpcClient,
): QueueAccessStore {
  return {
    async findRepo(fullName) {
      const { data, error } = await client
        .from("github_repositories")
        .select("id, owner, name, installation_id")
        .eq("full_name", fullName)
        .maybeSingle();
      if (error) throw new Error(`repository lookup failed: ${error.message}`);
      return (data as QueueRepo | null) ?? null;
    },
    async installationOwner(installationId) {
      const { data, error } = await client
        .from("github_app_installations")
        .select("linked_user_id")
        .eq("id", installationId)
        .maybeSingle();
      if (error) throw new Error(`installation lookup failed: ${error.message}`);
      return (data as { linked_user_id: string | null } | null)?.linked_user_id ?? null;
    },
    installationHasPro: (installationId) =>
      installationHasPro(client, installationId),
  };
}
