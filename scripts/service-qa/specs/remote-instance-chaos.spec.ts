/**
 * Chaos cases for `remote-instance` (races, stuck rows, hostile input)
 * against the local database. Runs `remote-instance/lib.ts` in this process
 * with the service-role client, as billing-enforcement-remote.spec.ts does;
 * the provider is the in-memory LocalProvider, wrapped to slow or fail calls.
 */
import { randomUUID } from "node:crypto";
import { expect, it } from "vitest";
import type { SupabaseClient } from "@supabase/supabase-js";
import * as lapseSweep from "../../../supabase/functions/_shared/remote/lapse-sweep";
import { LocalProvider } from "../../../supabase/functions/_shared/remote/local-provider";
import { ProviderError, type SandboxProvider } from "../../../supabase/functions/_shared/remote/provider";
import { handleRemoteInstanceAction } from "../../../supabase/functions/remote-instance/lib";
import { getServiceClient } from "../clients";
import { recordOutcome } from "../record";
import { createTestUser, deleteTestUser, signInWithEmailPassword } from "../seed";

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

async function setRow(userId: string, fields: Record<string, unknown>) {
  const { error } = await admin.from("remote_instances").update(fields).eq("owner_user_id", userId).neq("status", "deleted");
  if (error) throw new Error(error.message);
}

const minutesAgo = (m: number) => new Date(Date.now() - m * 60_000).toISOString();

it("ensure resumes a provisioning row older than the threshold, and a degraded one", async () => {
  const u = await createTestUser();
  try {
    expect((await ensure(u.user.id)).status).toBe(200);
    await setRow(u.user.id, { status: "provisioning", updated_at: minutesAgo(60) });
    const stuck = await ensure(u.user.id);
    expect(stuck.status).toBe(200);
    expect((stuck.json.instance as { status: string }).status).toBe("ready");

    await setRow(u.user.id, { status: "degraded" });
    const degraded = await ensure(u.user.id);
    expect(degraded.status).toBe(200);
    expect((degraded.json.instance as { status: string }).status).toBe("ready");

    await setRow(u.user.id, { status: "provisioning", updated_at: new Date().toISOString() });
    const fresh = await ensure(u.user.id);
    expect((fresh.json.instance as { status: string }).status).toBe("provisioning");

    await recordOutcome("remote-instance-chaos-03-resume-stuck", {
      expectations: [
        "Ensure on a 'provisioning' row untouched for an hour runs provisioning again and ends 'ready'.",
        "Ensure on a 'degraded' row ends 'ready'; a fresh 'provisioning' row is left to its own ensure.",
      ],
      details: { stuck: stuck.json.status, degraded: degraded.json.status },
    });
  } finally {
    await deleteTestUser(admin, u.user.id);
  }
}, 30_000);

it("ensure refuses an empty, overlong or control-character idempotency key with validation_error", async () => {
  const u = await createTestUser();
  try {
    for (const key of ["", "k".repeat(257), "a\u0000b", "line\nbreak"]) {
      const res = await ensure(u.user.id, undefined, { idempotency_key: key });
      expect(res.status, JSON.stringify(key)).toBe(400);
      expect(res.json.code).toBe("validation_error");
    }
    expect(await rows(admin, u.user.id)).toEqual([]);
    expect((await ensure(u.user.id, undefined, { idempotency_key: "k".repeat(256) })).status).toBe(200);

    await recordOutcome("remote-instance-chaos-04-idempotency-key", {
      expectations: [
        "Empty, 257-character and control-character keys answer 400 validation_error and create no row.",
        "A 256-character key is accepted.",
      ],
      details: {},
    });
  } finally {
    await deleteTestUser(admin, u.user.id);
  }
}, 30_000);

/** The service client, with inserts into remote_instance_operations failing. */
function failingOperationsInsert(): SupabaseClient {
  return new Proxy(admin, {
    get(target, prop, receiver) {
      if (prop !== "from") return Reflect.get(target, prop, receiver);
      return (table: string) => {
        const builder = target.from(table);
        if (table !== "remote_instance_operations") return builder;
        return new Proxy(builder, {
          get(b, p, r) {
            if (p !== "insert") return Reflect.get(b, p, r);
            return () => ({
              select: () => ({ single: async () => ({ data: null, error: { message: "connection reset" } }) }),
            });
          },
        });
      };
    },
  });
}

it("a failed operation insert after the instance row was created leaves no stuck provisioning row", async () => {
  const u = await createTestUser();
  try {
    const response = await handleRemoteInstanceAction(
      { action: "ensure", idempotency_key: `ensure-${randomUUID()}` },
      {
        supabase: failingOperationsInsert(),
        ownerUserId: u.user.id,
        correlationId: `sqa-${randomUUID()}`,
        hasPro: async () => true,
        stub: true,
        provider: (kind) => new LocalProvider(kind),
      },
    );
    expect(response.status).toBe(500);
    const after = await rows(admin, u.user.id);
    expect(after.map((r) => r.status)).not.toContain("provisioning");

    await recordOutcome("remote-instance-chaos-05-begin-operation-fails", {
      expectations: ["When recording the operation fails after the row was made, the row is not left 'provisioning'."],
      details: { after },
    });
  } finally {
    await deleteTestUser(admin, u.user.id);
  }
}, 30_000);

it("the sweep marks a provisioning row untouched past the threshold failed", async () => {
  const u = await createTestUser();
  const fresh = await createTestUser();
  try {
    expect((await ensure(u.user.id)).status).toBe(200);
    expect((await ensure(fresh.user.id)).status).toBe(200);
    await setRow(u.user.id, { status: "provisioning", updated_at: minutesAgo(60) });
    await setRow(fresh.user.id, { status: "provisioning", updated_at: new Date().toISOString() });

    const sweep = (lapseSweep as Record<string, unknown>).sweepStaleProvisioning as (
      supabase: SupabaseClient,
      now: Date,
    ) => Promise<string[]>;
    const failed = await sweep(admin, new Date());
    expect((await rows(admin, u.user.id)).map((r) => r.status)).toEqual(["failed"]);
    expect((await rows(admin, fresh.user.id)).map((r) => r.status)).toEqual(["provisioning"]);

    await recordOutcome("remote-instance-chaos-06-stale-sweep", {
      expectations: ["The sweep marks a 'provisioning' row an hour old failed and leaves a fresh one alone."],
      details: { failed },
    });
  } finally {
    await deleteTestUser(admin, u.user.id);
    await deleteTestUser(admin, fresh.user.id);
  }
}, 30_000);

it("concurrent ensures answer 200 and leave one live instance", async () => {
  const same = await createTestUser();
  const distinct = await createTestUser();
  try {
    const key = `ensure-${randomUUID()}`;
    const sameKey = await Promise.all(
      Array.from({ length: 20 }, () => call(same.user.id, { action: "ensure", idempotency_key: key })),
    );
    const distinctKeys = await Promise.all(Array.from({ length: 30 }, () => ensure(distinct.user.id)));
    expect(sameKey.map((r) => r.status)).toEqual(Array(20).fill(200));
    expect(distinctKeys.map((r) => r.status)).toEqual(Array(30).fill(200));
    expect((await rows(admin, same.user.id)).filter((r) => r.status !== "deleted")).toHaveLength(1);
    expect((await rows(admin, distinct.user.id)).filter((r) => r.status !== "deleted")).toHaveLength(1);

    await recordOutcome("remote-instance-chaos-07-concurrent-ensure", {
      expectations: [
        "20 parallel ensures with one key and 30 with distinct keys all answer 200.",
        "Each owner ends with exactly one live remote_instances row.",
      ],
      details: { sameKey: sameKey.map((r) => r.json.status), distinct: distinctKeys.map((r) => r.json.status) },
    });
  } finally {
    await deleteTestUser(admin, same.user.id);
    await deleteTestUser(admin, distinct.user.id);
  }
}, 60_000);

it("a provider error's secrets never reach rows the owner can read", async () => {
  const u = await createTestUser();
  try {
    const secret = "sk_live_SECRET";
    const url = "https://api.sprites.dev/internal";
    const chaos = chaosProvider(() => ({
      createInstance: async () => {
        throw new ProviderError("unavailable", `upstream 500 token=${secret} at ${url}`);
      },
    }));
    const res = await ensure(u.user.id, chaos.make);
    expect(res.status).toBe(502);

    const { client } = await signInWithEmailPassword(u.email, u.password);
    const ops = await client.from("remote_instance_operations").select("*");
    const audit = await client.from("remote_audit_events").select("*");
    expect(ops.error).toBeNull();
    expect(audit.error).toBeNull();
    expect(ops.data?.length).toBeGreaterThan(0);
    const readable = JSON.stringify({ ops: ops.data, audit: audit.data, res: res.json });
    expect(readable).not.toContain(secret);
    expect(readable).not.toContain("api.sprites.dev");
    expect(ops.data?.[0].error_message).toMatch(/^provider_unavailable/);

    await recordOutcome("remote-instance-chaos-08-scrubbed-errors", {
      expectations: [
        "A provider error carrying a token and an internal URL fails ensure with 502.",
        "Neither the token nor the URL appears in the operation or audit rows the owner reads with their JWT.",
        "The operation's error_message starts with the error code provider_unavailable.",
      ],
      details: { error_message: ops.data?.[0].error_message },
    });
  } finally {
    await deleteTestUser(admin, u.user.id);
  }
}, 30_000);

it("malformed requests answer 400 with a code and create nothing", async () => {
  const u = await createTestUser();
  try {
    const key = () => `k-${randomUUID()}`;
    const cases: Array<[string, unknown, number, string]> = [
      ["null body", null, 400, "validation_error"],
      ["array body", [], 400, "validation_error"],
      ["string body", "ensure", 400, "validation_error"],
      ["unknown region", { action: "ensure", idempotency_key: key(), region: "mars_north" }, 400, "validation_error"],
      ["unknown size", { action: "ensure", idempotency_key: key(), size_preset: "galactic" }, 400, "validation_error"],
      ["numeric instance_id", { action: "status", instance_id: 42 }, 400, "validation_error"],
      ["non-uuid instance_id", { action: "wake", idempotency_key: key(), instance_id: "../etc" }, 400, "validation_error"],
      ["missing key", { action: "wake" }, 400, "validation_error"],
      ["unknown action", { action: "explode" }, 400, "unknown_action"],
      ["missing action", {}, 400, "unknown_action"],
    ];
    for (const [name, body, status, code] of cases) {
      const res = await call(u.user.id, body);
      expect({ name, status: res.status, code: res.json.code }).toEqual({ name, status, code });
    }
    expect(await rows(admin, u.user.id)).toEqual([]);

    const huge = "x".repeat(100_000);
    const res = await call(u.user.id, { action: huge });
    expect(res.json.code).toBe("unknown_action");
    expect(JSON.stringify(res.json).length).toBeLessThan(500);

    await recordOutcome("remote-instance-chaos-09-input-validation", {
      expectations: [
        "Non-object bodies, unknown region/size_preset, non-uuid instance_id and a missing key answer 400 validation_error.",
        "An unknown or missing action answers 400 unknown_action, and a 100 kB action is not echoed back.",
        "None of these requests creates a remote_instances row.",
      ],
      details: { cases: cases.map(([name]) => name) },
    });
  } finally {
    await deleteTestUser(admin, u.user.id);
  }
}, 30_000);

it("wake and reprovision before a machine exists answer 409 no_provider_resource", async () => {
  const u = await createTestUser();
  try {
    const chaos = chaosProvider(() => ({
      createInstance: async () => {
        throw new ProviderError("unavailable", "down");
      },
    }));
    await ensure(u.user.id, chaos.make);
    for (const action of ["wake", "reprovision"]) {
      const res = await call(u.user.id, { action, idempotency_key: `${action}-${randomUUID()}` });
      expect({ action, status: res.status, code: res.json.code }).toEqual({
        action,
        status: 409,
        code: "no_provider_resource",
      });
    }
    await recordOutcome("remote-instance-chaos-10-no-provider-resource", {
      expectations: ["Wake and reprovision on an instance with no machine answer 409 with code no_provider_resource."],
      details: {},
    });
  } finally {
    await deleteTestUser(admin, u.user.id);
  }
}, 30_000);
