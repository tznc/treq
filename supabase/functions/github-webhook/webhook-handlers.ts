// Translates verified GitHub webhook payloads into merge queue commands.
//
// The webhook layer performs no orchestration: apart from upserting directly
// observable GitHub facts (installation/repository rows — fast, idempotent DB
// writes), every state change is expressed as a PGMQ command executed by the
// merge-queue-worker.

import type { SupabaseClient } from "npm:@supabase/supabase-js@2.95.3";
import type { MergeQueueCommand } from "../_shared/merge-queue/messages.ts";
import { isQueueTestBranch } from "../_shared/merge-queue/scheduler.ts";

function newCommandId(): string {
  return crypto.randomUUID();
}

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
  label?: { name?: string };
  pull_request?: {
    number: number;
    title?: string | null;
    merged?: boolean;
    user?: { login?: string } | null;
    base: { ref: string };
    head: { sha: string; ref?: string | null };
  };
  check_suite?: {
    conclusion?: string | null;
    head_branch?: string | null;
    head_sha?: string | null;
  };
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
    for (const r of payload.repositories_removed ?? []) {
      await supabase.from("github_repositories").delete().eq("id", r.id);
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

// ── Command builders ──────────────────────────────────────────────────────────

export async function commandsForEvent(
  supabase: SupabaseClient,
  event: string,
  payload: GitHubWebhookPayload,
  deliveryId: string,
): Promise<MergeQueueCommand[]> {
  switch (event) {
    case "installation":
    case "installation_repositories":
      return commandsForInstallation(event, payload, deliveryId);
    case "pull_request":
      return commandsForPullRequest(supabase, payload, deliveryId);
    case "check_suite":
      return commandsForCheckSuite(supabase, payload, deliveryId);
    default:
      return [];
  }
}

function commandsForInstallation(
  event: string,
  payload: GitHubWebhookPayload,
  deliveryId: string,
): MergeQueueCommand[] {
  const installation = payload.installation;
  if (!installation) return [];

  const action: "created" | "repositories_changed" | "deleted" | null =
    event === "installation_repositories"
      ? "repositories_changed"
      : payload.action === "created"
        ? "created"
        : payload.action === "deleted"
          ? "deleted"
          : null;
  if (!action) return [];

  return [
    {
      version: 1,
      type: "installation.sync",
      commandId: newCommandId(),
      deliveryId,
      installationId: installation.id,
      action,
      occurredAt: new Date().toISOString(),
    },
  ];
}

async function commandsForPullRequest(
  supabase: SupabaseClient,
  payload: GitHubWebhookPayload,
  deliveryId: string,
): Promise<MergeQueueCommand[]> {
  const action = payload.action as string;
  const pr = payload.pull_request;
  const repoPayload = payload.repository;
  if (!payload.installation?.id || !pr || !repoPayload) return [];

  const relevant = ["labeled", "unlabeled", "closed", "synchronize"];
  if (!relevant.includes(action)) return [];

  const { data: repoRow } = await supabase
    .from("github_repositories")
    .select("id")
    .eq("id", repoPayload.id)
    .maybeSingle();
  if (!repoRow) return []; // repo not tracked

  // Queue configuration is keyed by the PR's actual target branch.
  const targetBranch: string = pr.base.ref;
  const { data: configRow } = await supabase
    .from("merge_queue_configs")
    .select("queue_trigger_label, enabled")
    .eq("repo_id", repoRow.id)
    .eq("target_branch", targetBranch)
    .maybeSingle();

  const triggerLabel: string = configRow?.queue_trigger_label ?? "merge-queue";
  const queueEnabled: boolean = configRow?.enabled ?? true;
  if (!queueEnabled) return [];

  const labelName: string | undefined =
    action === "labeled" || action === "unlabeled"
      ? payload.label?.name
      : undefined;

  if (action === "labeled") {
    if (labelName !== triggerLabel) return [];
    const { data: queue, error } = await supabase.rpc("get_or_create_merge_queue", {
      p_repo_id: repoRow.id,
      p_target_branch: targetBranch,
    });
    if (error || !queue) {
      throw new Error(`failed to resolve queue: ${error?.message}`);
    }
    return [
      {
        version: 1,
        type: "entry.enqueue",
        commandId: newCommandId(),
        deliveryId,
        repositoryId: String(repoRow.id),
        queueId: queue.id,
        pullRequestNumber: pr.number,
        headSha: pr.head.sha,
        baseBranch: targetBranch,
        prTitle: pr.title ?? null,
        prAuthor: pr.user?.login ?? null,
        headRefName: pr.head.ref ?? null,
        occurredAt: new Date().toISOString(),
      },
    ];
  }

  // unlabeled / closed / synchronize only matter for PRs already queued.
  const { data: queueRow } = await supabase
    .from("merge_queues")
    .select("id")
    .eq("repo_id", repoRow.id)
    .eq("target_branch", targetBranch)
    .maybeSingle();
  if (!queueRow) return [];

  const { data: entryRow } = await supabase
    .from("merge_queue_entries")
    .select("id, status")
    .eq("queue_id", queueRow.id)
    .eq("pr_number", pr.number)
    .maybeSingle();
  if (!entryRow) return [];
  if (["merged", "failed", "dequeued"].includes(entryRow.status)) return [];

  if (action === "unlabeled" && labelName !== triggerLabel) return [];

  if (action === "synchronize") {
    return [
      {
        version: 1,
        type: "entry.dequeue",
        commandId: newCommandId(),
        deliveryId,
        repositoryId: String(repoRow.id),
        queueId: queueRow.id,
        pullRequestNumber: pr.number,
        reason: "synchronized",
        headSha: pr.head.sha,
        occurredAt: new Date().toISOString(),
      },
    ];
  }

  return [
    {
      version: 1,
      type: "entry.dequeue",
      commandId: newCommandId(),
      deliveryId,
      repositoryId: String(repoRow.id),
      queueId: queueRow.id,
      pullRequestNumber: pr.number,
      reason: action === "closed" ? "closed" : "label_removed",
      merged: action === "closed" ? Boolean(pr.merged) : undefined,
      occurredAt: new Date().toISOString(),
    },
  ];
}

async function commandsForCheckSuite(
  supabase: SupabaseClient,
  payload: GitHubWebhookPayload,
  deliveryId: string,
): Promise<MergeQueueCommand[]> {
  if (payload.action !== "completed") return [];

  const suite = payload.check_suite;
  const conclusion = suite?.conclusion ?? null;
  const headBranch = suite?.head_branch ?? null;
  const headSha = suite?.head_sha ?? null;
  if (!conclusion || !headSha) return [];
  if (!headBranch || !isQueueTestBranch(headBranch)) return [];

  const { data: ciRun } = await supabase
    .from("ci_runs")
    .select("id, queue_id, head_sha")
    .eq("head_sha", headSha)
    .in("status", ["pending", "running"])
    .maybeSingle();
  if (!ciRun) return [];

  return [
    {
      version: 1,
      type: "ci.completed",
      commandId: newCommandId(),
      deliveryId,
      queueId: ciRun.queue_id,
      ciRunId: ciRun.id,
      testedSha: headSha,
      conclusion,
      occurredAt: new Date().toISOString(),
    },
  ];
}
