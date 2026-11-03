/**
 * Chaos cases for `remote-instance` (races, stuck rows, hostile input)
 * against the local database. Runs `remote-instance/lib.ts` in this process
 * with the service-role client, as billing-enforcement-remote.spec.ts does;
 * the provider is the in-memory LocalProvider, wrapped to slow or fail calls.
 */
import { randomUUID } from "node:crypto";
import { expect, it } from "vitest";
import type { SupabaseClient } from "@supabase/supabase-js";
import { LocalProvider } from "../../../supabase/functions/_shared/remote/local-provider";
import type { SandboxProvider } from "../../../supabase/functions/_shared/remote/provider";
import { handleRemoteInstanceAction } from "../../../supabase/functions/remote-instance/lib";
import { getServiceClient } from "../clients";
import { recordOutcome } from "../record";
import { createTestUser, deleteTestUser } from "../seed";

const admin = getServiceClient();
const sleep = (ms: number) => new Promise((r) => setTimeout(r, ms));

type Hooks = Partial<Pick<SandboxProvider, "createInstance" | "deleteInstance" | "getInstance">>;

/** A LocalProvider whose calls can be slowed or replaced, recording deletes. */
function chaosProvider(hooks: (base: LocalProvider) => Hooks = () => ({})) {
  const deleted: string[] = [];
  const make = (kind: string) => {
    const base = new LocalProvider(kind as never);
    const h = hooks(base);
    return new Proxy(base, {
      get(target, prop, receiver) {
        if (prop === "deleteInstance") {
          return (id: string, opts?: unknown) => {
            deleted.push(id);
            return (h.deleteInstance ?? target.deleteInstance.bind(target))(id, opts as never);
          };
        }
        if (prop in h) return (h as Record<string | symbol, unknown>)[prop];
        return Reflect.get(target, prop, receiver);
      },
    }) as SandboxProvider;
  };
  return { make, deleted };
}

async function call(
  userId: string,
  body: unknown,
  provider: (kind: string) => SandboxProvider = (kind) => new LocalProvider(kind as never),
) {
  const response = await handleRemoteInstanceAction(body as Record<string, unknown>, {
    supabase: admin,
    ownerUserId: userId,
    correlationId: `sqa-${randomUUID()}`,
    hasPro: async () => true,
    stub: true,
    provider: provider as never,
  });
  return { status: response.status, json: (await response.json()) as Record<string, unknown> };
}

const ensure = (userId: string, provider?: (kind: string) => SandboxProvider, extra: Record<string, unknown> = {}) =>
  call(userId, { action: "ensure", idempotency_key: `ensure-${randomUUID()}`, ...extra }, provider);
const del = (userId: string) => call(userId, { action: "delete", idempotency_key: `delete-${randomUUID()}` });

async function rows(supabase: SupabaseClient, userId: string) {
  const { data, error } = await supabase
    .from("remote_instances")
    .select("id, status, provider_resource_id, created_at")
    .eq("owner_user_id", userId)
    .order("created_at", { ascending: true });
  if (error) throw new Error(error.message);
  return data as Array<{ id: string; status: string; provider_resource_id: string | null }>;
}

async function waitForStatus(userId: string, status: string, timeoutMs = 5_000) {
  const until = Date.now() + timeoutMs;
  while (Date.now() < until) {
    if ((await rows(admin, userId)).some((r) => r.status === status)) return;
    await sleep(25);
  }
  throw new Error(`no row reached ${status}`);
}

it("a delete during provisioning stays deleted and removes the machine ensure created", async () => {
  const u = await createTestUser();
  try {
    const chaos = chaosProvider((base) => ({
      createInstance: async (p) => {
        await sleep(800);
        return base.createInstance(p);
      },
    }));
    const ensuring = ensure(u.user.id, chaos.make);
    await sleep(300);
    const deleted = await del(u.user.id);
    expect(deleted.status).toBe(200);
    await ensuring;

    const after = await rows(admin, u.user.id);
    expect(after.map((r) => r.status)).toEqual(["deleted"]);
    expect(chaos.deleted).toContainEqual(expect.stringContaining(u.user.id));

    await recordOutcome("remote-instance-chaos-01-delete-during-provisioning", {
      expectations: [
        "A delete that lands while ensure waits on createInstance leaves the only row 'deleted'.",
        "Ensure deletes the machine it created once it sees the row was deleted.",
      ],
      details: { after, deletedMachines: chaos.deleted },
    });
  } finally {
    await deleteTestUser(admin, u.user.id);
  }
}, 30_000);

it("a delete during bootstrapping stays deleted", async () => {
  const u = await createTestUser();
  try {
    const chaos = chaosProvider((base) => ({
      getInstance: async (id) => {
        await sleep(800);
        return base.getInstance(id);
      },
    }));
    const ensuring = ensure(u.user.id, chaos.make);
    await waitForStatus(u.user.id, "bootstrapping");
    const deleted = await del(u.user.id);
    expect(deleted.status).toBe(200);
    await ensuring;

    const after = await rows(admin, u.user.id);
    expect(after.map((r) => r.status)).toEqual(["deleted"]);

    await recordOutcome("remote-instance-chaos-02-delete-during-bootstrapping", {
      expectations: ["A delete that lands while ensure is bootstrapping leaves the only row 'deleted'."],
      details: { after },
    });
  } finally {
    await deleteTestUser(admin, u.user.id);
  }
}, 30_000);
