// GitHub App authentication and the REST operations used by the merge queue.
// All GitHub side effects go through this adapter so they can be stubbed in
// tests and audited in one place.
//
// Required env vars:
//   GITHUB_APP_ID                 - numeric App ID
//   GITHUB_APP_PRIVATE_KEY_BASE64 - base64-encoded PEM private key

import { fetchWithTimeout } from "../http/fetch-with-timeout.ts";

function base64url(data: ArrayBuffer | string): string {
  const bytes =
    typeof data === "string" ? new TextEncoder().encode(data) : new Uint8Array(data);
  let binary = "";
  for (const byte of bytes) binary += String.fromCharCode(byte);
  return btoa(binary).replace(/\+/g, "-").replace(/\//g, "_").replace(/=/g, "");
}

function pemToDer(pem: string): ArrayBuffer {
  const b64 = pem.replace(/-----[A-Z ]+-----/g, "").replace(/\s/g, "");
  const binary = atob(b64);
  const bytes = new Uint8Array(binary.length);
  for (let i = 0; i < binary.length; i++) bytes[i] = binary.charCodeAt(i);
  return bytes.buffer;
}

async function createAppJWT(appId: string, privateKeyPem: string): Promise<string> {
  const now = Math.floor(Date.now() / 1000);
  const header = base64url(JSON.stringify({ alg: "RS256", typ: "JWT" }));
  const payload = base64url(
    JSON.stringify({ iat: now - 60, exp: now + 540, iss: appId }),
  );
  const signingInput = `${header}.${payload}`;

  const key = await crypto.subtle.importKey(
    "pkcs8",
    pemToDer(privateKeyPem),
    { name: "RSASSA-PKCS1-v1_5", hash: "SHA-256" },
    false,
    ["sign"],
  );

  const sig = await crypto.subtle.sign(
    "RSASSA-PKCS1-v1_5",
    key,
    new TextEncoder().encode(signingInput),
  );

  return `${signingInput}.${base64url(sig)}`;
}

function appCredentials(): { appId: string; privateKey: string } {
  const appId = Deno.env.get("GITHUB_APP_ID") ?? "";
  const keyB64 = Deno.env.get("GITHUB_APP_PRIVATE_KEY_BASE64") ?? "";
  if (!appId || !keyB64) {
    throw new Error(
      "GITHUB_APP_ID and GITHUB_APP_PRIVATE_KEY_BASE64 must be configured",
    );
  }
  return { appId, privateKey: atob(keyB64) };
}

export async function getInstallationToken(installationId: number): Promise<string> {
  const { appId, privateKey } = appCredentials();
  const jwt = await createAppJWT(appId, privateKey);

  const res = await fetchWithTimeout(
    `https://api.github.com/app/installations/${installationId}/access_tokens`,
    {
      method: "POST",
      headers: {
        Authorization: `Bearer ${jwt}`,
        Accept: "application/vnd.github+json",
        "X-GitHub-Api-Version": "2022-11-28",
        "User-Agent": "treq-merge-queue/1.0",
      },
    },
  );

  if (!res.ok) {
    throw new Error(
      `Failed to get installation token for ${installationId}: ${res.status} ${await res.text()}`,
    );
  }
  return (await res.json()).token;
}

// Fetch installation metadata using app-level (JWT) auth. Used by the
// installation linking flow to verify an installation actually exists and
// which account it belongs to.
export async function getInstallation(installationId: number): Promise<{
  id: number;
  account: { login: string; type: string; avatar_url: string | null };
  app_id: number;
} | null> {
  const { appId, privateKey } = appCredentials();
  const jwt = await createAppJWT(appId, privateKey);

  const res = await fetchWithTimeout(
    `https://api.github.com/app/installations/${installationId}`,
    {
      headers: {
        Authorization: `Bearer ${jwt}`,
        Accept: "application/vnd.github+json",
        "X-GitHub-Api-Version": "2022-11-28",
        "User-Agent": "treq-merge-queue/1.0",
      },
    },
  );

  if (res.status === 404) return null;
  if (!res.ok) {
    throw new Error(
      `Failed to fetch installation ${installationId}: ${res.status} ${await res.text()}`,
    );
  }
  return res.json();
}

export interface PullRequestSnapshot {
  number: number;
  state: string; // open | closed
  merged: boolean;
  title: string;
  baseRef: string;
  headRef: string;
  headSha: string;
}

export interface GitHubAdapter {
  getBranchSha(branch: string): Promise<string>;
  createOrResetBranch(name: string, sha: string): Promise<void>;
  deleteBranch(name: string): Promise<void>;
  mergeInto(base: string, head: string, message: string): Promise<string | null>;
  getPR(prNumber: number): Promise<PullRequestSnapshot | null>;
  mergePR(
    prNumber: number,
    mergeMethod: string,
    commitTitle: string,
    expectedHeadSha: string,
  ): Promise<void>;
  createComment(prNumber: number, body: string, marker?: string): Promise<void>;
  hasCommentWithMarker(prNumber: number, marker: string): Promise<boolean>;
  addLabel(prNumber: number, labels: string[]): Promise<void>;
  removeLabel(prNumber: number, label: string): Promise<void>;
  listCheckRuns(ref: string): Promise<
    Array<{ name: string; status: string; conclusion: string | null }>
  >;
}

const TREQ_COMMENT_FOOTER =
  "<sub>[Treq](https://treq.dev/?utm_source=github&utm_medium=merge_queue_comment&utm_campaign=merge_queue) runs this merge queue.</sub>";

// Reviewers who don't use Treq see these comments, so each one links to the
// site. The optional invisible marker stays last so a redelivered command can
// find its earlier comment and not repeat itself.
export function formatCommentBody(body: string, marker?: string): string {
  const withFooter = `${body}\n\n${TREQ_COMMENT_FOOTER}`;
  return marker ? `${withFooter}\n\n<!-- treq:${marker} -->` : withFooter;
}

export class GitHubRestAdapter implements GitHubAdapter {
  constructor(
    private readonly token: string,
    private readonly owner: string,
    private readonly repo: string,
  ) {}

  static async forInstallation(
    installationId: number,
    owner: string,
    repo: string,
  ): Promise<GitHubRestAdapter> {
    const token = await getInstallationToken(installationId);
    return new GitHubRestAdapter(token, owner, repo);
  }

  private async request<T = unknown>(
    path: string,
    method = "GET",
    body?: unknown,
  ): Promise<{ status: number; data: T | null }> {
    const url = `https://api.github.com/repos/${this.owner}/${this.repo}${path}`;
    const res = await fetchWithTimeout(url, {
      method,
      headers: {
        Authorization: `Bearer ${this.token}`,
        Accept: "application/vnd.github+json",
        "Content-Type": "application/json",
        "X-GitHub-Api-Version": "2022-11-28",
        "User-Agent": "treq-merge-queue/1.0",
      },
      body: body !== undefined ? JSON.stringify(body) : undefined,
    });

    if (res.status === 204 || res.status === 404) {
      // Drain the body so the connection can be reused.
      await res.body?.cancel();
      return { status: res.status, data: null };
    }
    if (!res.ok) {
      const text = await res.text();
      // The status code in this message is parsed by classifyError.
      throw new Error(`GitHub ${method} ${url} → ${res.status}: ${text}`);
    }
    return { status: res.status, data: (await res.json()) as T };
  }

  async getBranchSha(branch: string): Promise<string> {
    const { data } = await this.request<{ commit: { sha: string } }>(
      `/branches/${encodeURIComponent(branch)}`,
    );
    if (!data) throw new Error(`Branch not found: ${branch}`);
    return data.commit.sha;
  }

  // Creating an existing branch is fine when it already points at the
  // expected SHA (idempotent retry); otherwise it is force-reset.
  async createOrResetBranch(name: string, sha: string): Promise<void> {
    try {
      await this.request("/git/refs", "POST", {
        ref: `refs/heads/${name}`,
        sha,
      });
    } catch (err) {
      const message = err instanceof Error ? err.message : String(err);
      if (!/→ 422:/.test(message)) throw err;
      // Reference already exists — reset it to the requested base.
      await this.request(`/git/refs/heads/${encodeURIComponent(name)}`, "PATCH", {
        sha,
        force: true,
      });
    }
  }

  // "Branch not found" is treated as successful deletion.
  async deleteBranch(name: string): Promise<void> {
    try {
      await this.request(`/git/refs/heads/${encodeURIComponent(name)}`, "DELETE");
    } catch (err) {
      const message = err instanceof Error ? err.message : String(err);
      if (/→ 422:.*(does not exist|Reference does not exist)/is.test(message)) return;
      throw err;
    }
  }

  async mergeInto(base: string, head: string, message: string): Promise<string | null> {
    const { data } = await this.request<{ sha: string }>("/merges", "POST", {
      base,
      head,
      commit_message: message,
    });
    return data?.sha ?? null;
  }

  async getPR(prNumber: number): Promise<PullRequestSnapshot | null> {
    const { data } = await this.request<{
      number: number;
      state: string;
      merged: boolean;
      title: string;
      base: { ref: string };
      head: { ref: string; sha: string };
    }>(`/pulls/${prNumber}`);
    if (!data) return null;
    return {
      number: data.number,
      state: data.state,
      merged: data.merged,
      title: data.title,
      baseRef: data.base.ref,
      headRef: data.head.ref,
      headSha: data.head.sha,
    };
  }

  // expectedHeadSha is forwarded to GitHub, which rejects the merge with 409
  // if the PR head moved after CI tested it.
  async mergePR(
    prNumber: number,
    mergeMethod: string,
    commitTitle: string,
    expectedHeadSha: string,
  ): Promise<void> {
    await this.request(`/pulls/${prNumber}/merge`, "PUT", {
      merge_method: mergeMethod,
      commit_title: commitTitle,
      sha: expectedHeadSha,
    });
  }

  async createComment(prNumber: number, body: string, marker?: string): Promise<void> {
    await this.request(`/issues/${prNumber}/comments`, "POST", {
      body: formatCommentBody(body, marker),
    });
  }

  async hasCommentWithMarker(prNumber: number, marker: string): Promise<boolean> {
    const { data } = await this.request<Array<{ body: string }>>(
      `/issues/${prNumber}/comments?per_page=100`,
    );
    const needle = `<!-- treq:${marker} -->`;
    return (data ?? []).some((c) => c.body?.includes(needle));
  }

  async addLabel(prNumber: number, labels: string[]): Promise<void> {
    await this.request(`/issues/${prNumber}/labels`, "POST", { labels });
  }

  async removeLabel(prNumber: number, label: string): Promise<void> {
    try {
      await this.request(
        `/issues/${prNumber}/labels/${encodeURIComponent(label)}`,
        "DELETE",
      );
    } catch {
      // Label may not exist — removing an absent label is success.
    }
  }

  async listCheckRuns(
    ref: string,
  ): Promise<Array<{ name: string; status: string; conclusion: string | null }>> {
    const { data } = await this.request<{
      check_runs: Array<{ name: string; status: string; conclusion: string | null }>;
    }>(`/commits/${encodeURIComponent(ref)}/check-runs?per_page=100`);
    return data?.check_runs ?? [];
  }
}
