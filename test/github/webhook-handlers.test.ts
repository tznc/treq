import type { SupabaseClient } from "@supabase/supabase-js";
import { describe, expect, it } from "vitest";
import {
  applyInstallationFacts,
  type GitHubWebhookPayload,
} from "../../supabase/functions/github-webhook/webhook-handlers.ts";

interface Call {
  table: string;
  op: string;
  column?: string;
  value?: unknown;
}

// Records each query builder chain the handler runs; every call resolves to
// `result`.
function fakeSupabase(result: { error: { message: string } | null } = { error: null }) {
  const calls: Call[] = [];
  const client = {
    from(table: string) {
      const builder = (op: string) => {
        const call: Call = { table, op };
        calls.push(call);
        const chain = {
          eq(column: string, value: unknown) {
            Object.assign(call, { column, value });
            return Promise.resolve(result);
          },
          in(column: string, value: unknown) {
            Object.assign(call, { column, value });
            return Promise.resolve(result);
          },
        };
        return chain;
      };
      return {
        delete: () => builder("delete"),
        update: () => builder("update"),
        upsert: () => {
          calls.push({ table, op: "upsert" });
          return Promise.resolve(result);
        },
      };
    },
  };
  return { client: client as unknown as SupabaseClient, calls };
}

function removed(count: number): GitHubWebhookPayload {
  return {
    action: "removed",
    installation: { id: 7, app_id: 1, account: { login: "acme", type: "Organization" } },
    repositories_removed: Array.from({ length: count }, (_, i) => ({ id: 100 + i })),
  };
}

describe("installation_repositories removed", () => {
  it.each([1, 10, 100])("deletes %i repositories in one round trip", async (count) => {
    const { client, calls } = fakeSupabase();
    await applyInstallationFacts(client, "installation_repositories", removed(count));
    const deletes = calls.filter((c) => c.op === "delete");
    expect(deletes).toEqual([
      {
        table: "github_repositories",
        op: "delete",
        column: "id",
        value: Array.from({ length: count }, (_, i) => 100 + i),
      },
    ]);
  });

  it("sends no delete when nothing was removed", async () => {
    const { client, calls } = fakeSupabase();
    await applyInstallationFacts(client, "installation_repositories", removed(0));
    expect(calls.filter((c) => c.op === "delete")).toEqual([]);
  });

  it("throws when the delete fails, so the delivery is retried", async () => {
    const { client } = fakeSupabase({ error: { message: "boom" } });
    await expect(
      applyInstallationFacts(client, "installation_repositories", removed(2)),
    ).rejects.toThrow("boom");
  });
});
