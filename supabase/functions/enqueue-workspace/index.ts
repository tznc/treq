// Edge function: add or remove a workspace from the merge queue by adding /
// removing the queue trigger label on its GitHub PR.
//
// POST body: { repo_full_name: string; branch_name: string; action: "enqueue" | "dequeue" }
// Auth: user JWT in Authorization header (passed automatically by supabase.functions.invoke)
// "enqueue" requires Pro for the installation's owner (402 pro_required);
// "dequeue" always works. See lib.ts.

import { createClient } from "npm:@supabase/supabase-js@2.95.3";
import { fetchWithTimeout } from "../_shared/http/fetch-with-timeout.ts";
import { getInstallationToken } from "../_shared/merge-queue/github-adapter.ts";
import { authorizeQueueAction, queueAccessStore } from "./lib.ts";

const corsHeaders = {
  "Access-Control-Allow-Origin": "*",
  "Access-Control-Allow-Headers":
    "authorization, x-client-info, apikey, content-type",
};

Deno.serve(async (req) => {
  if (req.method === "OPTIONS") {
    return new Response(null, { headers: corsHeaders, status: 204 });
  }
  if (req.method !== "POST") {
    return json({ error: "Method not allowed" }, 405);
  }

  // Authenticate the caller
  const authHeader = req.headers.get("authorization") ?? "";
  const userToken = authHeader.replace(/^Bearer\s+/i, "");
  if (!userToken) return json({ error: "Unauthorized" }, 401);

  const supabaseUrl = Deno.env.get("SUPABASE_URL") ?? "";
  const supabaseAnonKey = Deno.env.get("SUPABASE_ANON_KEY") ?? "";
  const supabaseServiceKey = Deno.env.get("SUPABASE_SERVICE_ROLE_KEY") ?? "";

  // Verify user identity via their JWT
  const supabaseUser = createClient(supabaseUrl, supabaseAnonKey, {
    global: { headers: { Authorization: `Bearer ${userToken}` } },
  });
  const {
    data: { user },
    error: authError,
  } = await supabaseUser.auth.getUser();
  if (authError || !user) return json({ error: "Unauthorized" }, 401);

  // Parse request body
  let body: { repo_full_name?: string; branch_name?: string; action?: string };
  try {
    body = await req.json();
  } catch {
    return json({ error: "Invalid JSON" }, 400);
  }

  const { repo_full_name, branch_name, action } = body;
  if (!repo_full_name || !branch_name || !action) {
    return json({ error: "Missing required fields: repo_full_name, branch_name, action" }, 400);
  }
  if (action !== "enqueue" && action !== "dequeue") {
    return json({ error: "action must be 'enqueue' or 'dequeue'" }, 400);
  }

  const supabase = createClient(supabaseUrl, supabaseServiceKey);

  // Look up the repo + installation, verifying the user owns it and, to
  // enqueue, that its owner has Pro.
  let access: Awaited<ReturnType<typeof authorizeQueueAction>>;
  try {
    access = await authorizeQueueAction(
      { userId: user.id, repoFullName: repo_full_name, action },
      queueAccessStore(supabase),
    );
  } catch (err) {
    console.error("[enqueue-workspace] access check failed:", (err as Error).message);
    return json({ error: "Failed to check access" }, 500);
  }
  if (!access.ok) return json(access.body, access.status);
  const repoRow = access.repo;

  // Get an installation token to make GitHub API calls
  let token: string;
  try {
    token = await getInstallationToken(repoRow.installation_id);
  } catch (err) {
    console.error("[enqueue-workspace] installation token failed:", (err as Error).message);
    return json({ error: "Failed to authenticate with GitHub" }, 502);
  }

  const ghHeaders = {
    Authorization: `Bearer ${token}`,
    Accept: "application/vnd.github+json",
    "Content-Type": "application/json",
    "X-GitHub-Api-Version": "2022-11-28",
    "User-Agent": "treq-merge-queue/1.0",
  };

  const apiBase = `https://api.github.com/repos/${repoRow.owner}/${repoRow.name}`;

  // Find the open PR for this branch first: its base branch determines which
  // merge queue configuration applies.
  const prRes = await fetchWithTimeout(
    `${apiBase}/pulls?head=${encodeURIComponent(`${repoRow.owner}:${branch_name}`)}&state=open&per_page=5`,
    { headers: ghHeaders }
  );

  if (!prRes.ok) {
    console.error(`[enqueue-workspace] PR lookup failed: ${prRes.status} ${await prRes.text()}`);
    return json({ error: `GitHub API error looking up PR (HTTP ${statusClass(prRes.status)})` }, 502);
  }

  const prs: { number: number; head: { ref: string }; base: { ref: string } }[] =
    await prRes.json();
  const pr = prs.find((p) => p.head.ref === branch_name);

  if (!pr) {
    return json(
      { error: `No open PR found for branch '${branch_name}'. Push the branch and open a PR first.` },
      404
    );
  }

  // Queue config is keyed by (repo, target branch) — use the PR's actual base.
  const { data: configRow } = await supabase
    .from("merge_queue_configs")
    .select("queue_trigger_label, enabled")
    .eq("repo_id", repoRow.id)
    .eq("target_branch", pr.base.ref)
    .maybeSingle();

  if (configRow && !configRow.enabled && action === "enqueue") {
    return json(
      { error: `Merge queue is disabled for target branch '${pr.base.ref}'` },
      409
    );
  }

  const triggerLabel: string = configRow?.queue_trigger_label ?? "merge-queue";

  if (action === "enqueue") {
    const labelRes = await fetchWithTimeout(`${apiBase}/issues/${pr.number}/labels`, {
      method: "POST",
      headers: ghHeaders,
      body: JSON.stringify({ labels: [triggerLabel] }),
    });
    if (!labelRes.ok && labelRes.status !== 422) {
      console.error(`[enqueue-workspace] add label failed: ${labelRes.status} ${await labelRes.text()}`);
      return json({ error: `Failed to add label (HTTP ${statusClass(labelRes.status)})` }, 502);
    }
  } else {
    const labelRes = await fetchWithTimeout(
      `${apiBase}/issues/${pr.number}/labels/${encodeURIComponent(triggerLabel)}`,
      { method: "DELETE", headers: ghHeaders }
    );
    // 404 = label not on PR, which is fine
    if (!labelRes.ok && labelRes.status !== 404) {
      console.error(`[enqueue-workspace] remove label failed: ${labelRes.status} ${await labelRes.text()}`);
      return json({ error: `Failed to remove label (HTTP ${statusClass(labelRes.status)})` }, 502);
    }
  }

  return json({ ok: true, pr_number: pr.number, action }, 200);
});

// "4xx" / "5xx": enough for a client to tell its own mistake from an outage
// without exposing the upstream status or body.
function statusClass(status: number): string {
  return `${Math.floor(status / 100)}xx`;
}

function json(body: unknown, status = 200): Response {
  return new Response(JSON.stringify(body), {
    status,
    headers: { ...corsHeaders, "Content-Type": "application/json" },
  });
}
