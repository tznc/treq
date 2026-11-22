import { createClient } from "@supabase/supabase-js";
import { describe, expect, it } from "vitest";
import {
  applyInstallationFacts,
  type GitHubWebhookPayload,
} from "../../supabase/functions/github-webhook/webhook-handlers.ts";

interface Call {
  table: string;
  op: string;
  filter?: string;
}

const OPS: Record<string, string> = {
  DELETE: "delete",
  PATCH: "update",
  POST: "upsert",
};

// A real client whose requests reach a fake PostgREST: each request is
// recorded as the call that made it (a filter reads `column=op.value`), and
// every one answers `result`.
function fakeSupabase(
  result: { error: { message: string } | null } = { error: null },
) {
  const calls: Call[] = [];
  const fetch: typeof globalThis.fetch = async (input, init) => {
    const url = new URL(input instanceof Request ? input.url : input);
    const call: Call = {
      table: url.pathname.split("/").pop() ?? "",
      op: OPS[init?.method ?? "GET"] ?? "select",
    };
    for (const [column, value] of url.searchParams) {
      if (/^[a-z]+\./.test(value)) call.filter = `${column}=${value}`;
    }
    calls.push(call);
    return result.error
      ? Response.json({ message: result.error.message }, { status: 400 })
      : new Response(null, { status: 204 });
  };
  const client = createClient("http://postgrest.test", "service-key", {
    auth: { persistSession: false, autoRefreshToken: false },
    global: { fetch },
  });
  return { client, calls };
}

function removed(count: number): GitHubWebhookPayload {
  return {
    action: "removed",
    installation: {
      id: 7,
      app_id: 1,
      account: { login: "acme", type: "Organization" },
    },
    repositories_removed: Array.from({ length: count }, (_, i) => ({
      id: 100 + i,
    })),
  };
}

describe("installation_repositories removed", () => {
  it.each([
    1, 10, 100,
  ])("deletes %i repositories in one round trip", async (count) => {
    const { client, calls } = fakeSupabase();
    await applyInstallationFacts(
      client,
      "installation_repositories",
      removed(count),
    );
    const deletes = calls.filter((c) => c.op === "delete");
    const ids = Array.from({ length: count }, (_, i) => 100 + i);
    expect(deletes).toEqual([
      {
        table: "github_repositories",
        op: "delete",
        filter: `id=in.(${ids.join(",")})`,
      },
    ]);
  });

  it("sends no delete when nothing was removed", async () => {
    const { client, calls } = fakeSupabase();
    await applyInstallationFacts(
      client,
      "installation_repositories",
      removed(0),
    );
    expect(calls.filter((c) => c.op === "delete")).toEqual([]);
  });

  it("throws when the delete fails, so the delivery is retried", async () => {
    const { client } = fakeSupabase({ error: { message: "boom" } });
    await expect(
      applyInstallationFacts(client, "installation_repositories", removed(2)),
    ).rejects.toThrow("boom");
  });
});
