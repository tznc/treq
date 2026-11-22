// Local unit tests for instance-store lookups. A real client talks to a fake
// PostgREST that applies the eq/neq filters to in-memory rows, so no Supabase
// project is needed and supabase-js keeps its own maybeSingle rules.
//
// Run with: `deno test supabase/functions/tests/remote_instance_store.test.ts`

import { assertEquals } from "https://deno.land/std@0.224.0/assert/mod.ts";
import { createClient, type SupabaseClient } from "npm:@supabase/supabase-js@2.95.3";
import { getInstanceForOwner } from "../_shared/remote/instance-store.ts";

type Row = Record<string, unknown>;

// PostgREST filters read `column=op.value`, for example `status=neq.deleted`.
function passes(row: Row, column: string, filter: string): boolean {
  const dot = filter.indexOf(".");
  const op = filter.slice(0, dot);
  const value = filter.slice(dot + 1);
  if (op === "eq") return String(row[column]) === value;
  if (op === "neq") return String(row[column]) !== value;
  throw new Error(`fake PostgREST has no ${op} filter`);
}

function fakeClient(rows: Row[]): SupabaseClient {
  const fetch: typeof globalThis.fetch = (input) => {
    const url = new URL(input instanceof Request ? input.url : input);
    const filters = [...url.searchParams].filter(([column]) => column !== "select");
    const matching = rows.filter((row) => filters.every(([column, filter]) => passes(row, column, filter)));
    return Promise.resolve(Response.json(matching));
  };
  return createClient("http://postgrest.test", "anon-key", {
    auth: { persistSession: false, autoRefreshToken: false },
    global: { fetch },
  });
}

const deleted = { id: "old", owner_user_id: "u1", status: "deleted" };
const live = { id: "new", owner_user_id: "u1", status: "ready" };

Deno.test("an owner whose only instance was deleted has no instance", async () => {
  assertEquals(await getInstanceForOwner(fakeClient([deleted]), "u1"), null);
});

Deno.test("the recreated instance wins over deleted history", async () => {
  const instance = await getInstanceForOwner(
    fakeClient([deleted, { ...deleted, id: "older" }, live]),
    "u1",
  );
  assertEquals(instance?.id, "new");
});
