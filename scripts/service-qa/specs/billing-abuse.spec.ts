/**
 * Bad actors against billing and metering, through the local stack as a
 * signed-in user (PostgREST, RLS, grants) plus the remote-instance and relay
 * handlers in-process against the real database.
 */
import { randomUUID } from "node:crypto";
import { beforeAll, expect, it } from "vitest";
import { userHasPro } from "../../../supabase/functions/_shared/billing/entitlement";
import { spendStatusFor } from "../../../supabase/functions/_shared/billing/spend-cap";
import { LocalProvider } from "../../../supabase/functions/_shared/remote/local-provider";
import {
  authorizeRelayRequest,
  RelayError,
} from "../../../supabase/functions/_shared/remote/ssh-relay";
import { handleRemoteInstanceAction } from "../../../supabase/functions/remote-instance/lib";
import { clearBilling, grantPro } from "../billing";
import { getAnonClient, getServiceClient } from "../clients";
import { recordOutcome } from "../record";
import {
  createTestUser,
  deleteTestUser,
  signInWithEmailPassword,
} from "../seed";

const admin = getServiceClient();

// Seed prices take effect at `db reset`. These specs backdate usage by hours,
// which needs a price in effect then, so open prices start 30 days back.
beforeAll(async () => {
  const { error } = await admin
    .from("machine_type_prices")
    .update({
      effective_from: new Date(Date.now() - 30 * 86_400_000).toISOString(),
    })
    .is("effective_to", null)
    .gt("effective_from", new Date(Date.now() - 30 * 86_400_000).toISOString());
  if (error) throw new Error(error.message);
});

it("refuses every direct write to the billing ledger and spend tables", async () => {
  const u = await createTestUser();
  try {
    const { client } = await signInWithEmailPassword(u.email, u.password);
    const anon = getAnonClient();
    const id = u.user.id;
    const attempts: Record<string, string | null> = {};
    for (const [who, c] of [
      ["user", client],
      ["anon", anon],
    ] as const) {
      const tries: Array<
        [
          string,
          PromiseLike<{ error: { code?: string; message: string } | null }>,
        ]
      > = [
        [
          "usage_intervals.insert",
          c.from("usage_intervals").insert({
            placement_id: randomUUID(),
            owner_user_id: id,
            kind: "running",
            price_id: randomUUID(),
          }),
        ],
        [
          "usage_reports.insert",
          c.from("usage_reports").insert({
            identifier: `x-${who}`,
            owner_user_id: id,
            hour: new Date().toISOString(),
            seconds: 0,
            provider_cost_nanousd: 0,
            billed_nanousd: -1e12,
          }),
        ],
        [
          "usage_reports.update",
          c
            .from("usage_reports")
            .update({ billed_nanousd: 0 })
            .eq("owner_user_id", id),
        ],
        [
          "usage_reports.delete",
          c.from("usage_reports").delete().eq("owner_user_id", id),
        ],
        [
          "spend_caps.upsert",
          c.from("spend_caps").upsert({ user_id: id, cap_usd: 1e9 }),
        ],
        [
          "billing_subscriptions.insert",
          c.from("billing_subscriptions").insert({
            stripe_subscription_id: `sub_${who}`,
            stripe_customer_id: "cus_x",
            owner_type: "user",
            owner_id: id,
            plan: "pro",
            status: "active",
            stripe_event_created: new Date().toISOString(),
          }),
        ],
        [
          "subscriptions.update",
          c
            .from("subscriptions")
            .update({ plan: "pro", status: "active" })
            .neq("plan", "x"),
        ],
        [
          "machine_type_prices.update",
          c
            .from("machine_type_prices")
            .update({ running_nanousd_per_sec: 0 })
            .neq("running_nanousd_per_sec", -1),
        ],
        [
          "remote_instance_placements.update",
          c
            .from("remote_instance_placements")
            .update({ credential_source: "byos" })
            .eq("owner_user_id", id),
        ],
        [
          "set_placement_state",
          c.rpc("set_placement_state", {
            p_placement_id: randomUUID(),
            p_state: "suspended",
          }),
        ],
        [
          "build_usage_reports",
          c.rpc("build_usage_reports", { p_until: new Date().toISOString() }),
        ],
        [
          "set_machine_type_price",
          c.rpc("set_machine_type_price", {
            p_machine_type_id: "e2b:s",
            p_running_nanousd_per_sec: 0,
          }),
        ],
        [
          "get_spend_status_for",
          c.rpc("get_spend_status_for", { p_user_id: id }),
        ],
        [
          "record_user_notification",
          c.rpc("record_user_notification", {
            p_user_id: id,
            p_kind: "x",
            p_period: "x",
            p_title: "x",
            p_body: "x",
          }),
        ],
      ];
      for (const [name, q] of tries)
        attempts[`${who}.${name}`] = (await q).error?.code ?? null;
    }
    const allowed = Object.entries(attempts)
      .filter(([, code]) => code === null)
      .map(([k]) => k);
    // An update/delete that RLS filters to zero rows returns no error; prove nothing changed instead.
    const { data: prices } = await admin
      .from("machine_type_prices")
      .select("id")
      .eq("running_nanousd_per_sec", 0);
    expect(prices ?? []).toEqual([]);
    const { data: subs } = await admin
      .from("billing_subscriptions")
      .select("stripe_subscription_id")
      .eq("owner_id", id);
    expect(subs ?? []).toEqual([]);
    expect(await userHasPro(admin, id)).toBe(false);
    expect(
      allowed.filter(
        (k) =>
          !/\.(usage_reports\.(update|delete)|subscriptions\.update|machine_type_prices\.update|remote_instance_placements\.update)$/.test(
            k,
          ),
      ),
    ).toEqual([]);

    await recordOutcome("billing-abuse-01-direct-writes", {
      expectations: [
        "anon and a signed-in user cannot insert usage, reports, spend caps, subscriptions or call service-only billing RPCs.",
        "Updates/deletes on usage_reports, prices, placements and the subscriptions view change nothing.",
        "The user is still not Pro afterwards.",
      ],
      details: { attempts, allowedButNoEffect: allowed },
    });
  } finally {
    await deleteTestUser(admin, u.user.id);
  }
}, 60000);

it("validates spend caps against hostile numbers", async () => {
  const u = await createTestUser();
  try {
    const { client } = await signInWithEmailPassword(u.email, u.password);
    const inputs: unknown[] = [
      null,
      "NaN",
      "Infinity",
      "-Infinity",
      -1,
      0,
      4.994,
      4.995,
      4.999,
      5,
      5.004,
      999.999,
      1000,
      1000.004,
      1000.005,
      1e30,
      "1e-400",
      "abc",
      "50; drop table spend_caps",
    ];
    const results: Record<string, string> = {};
    for (const v of inputs) {
      const { data, error } = await client.rpc("set_spend_cap", {
        p_cap_usd: v,
      });
      results[String(v)] = error
        ? `err ${error.message}`
        : `ok ${(data as { cap_usd: number }).cap_usd}`;
    }
    const caps = Object.values(results)
      .filter((r) => r.startsWith("ok"))
      .map((r) => Number(r.slice(3)));
    for (const c of caps) {
      expect(c).toBeGreaterThanOrEqual(5);
      expect(c).toBeLessThanOrEqual(1000);
    }
    const { data: row } = await admin
      .from("spend_caps")
      .select("cap_usd")
      .eq("user_id", u.user.id)
      .single();
    expect(Number((row as { cap_usd: number }).cap_usd)).toBeGreaterThanOrEqual(
      5,
    );

    await recordOutcome("billing-abuse-02-spend-cap-fuzz", {
      expectations: [
        "NaN, ±Infinity, negatives, sub-$5, over-$1000, 1e30, tiny exponents and strings are refused.",
        "Every accepted cap lands in [$5, $1000] after rounding (4.995 and 1000.004 included).",
      ],
      details: { results },
    });
  } finally {
    await admin.from("spend_caps").delete().eq("user_id", u.user.id);
    await deleteTestUser(admin, u.user.id);
  }
}, 60000);

it("keeps usage summaries and notifications private", async () => {
  const a = await createTestUser();
  const b = await createTestUser();
  try {
    await admin.from("usage_reports").insert({
      identifier: `${a.user.id}:sqa`,
      owner_user_id: a.user.id,
      hour: "2026-10-01T10:00:00Z",
      seconds: 3600,
      provider_cost_nanousd: 1,
      billed_nanousd: 123_000_000,
      stripe_status: "zero",
    });
    await admin.rpc("record_user_notification", {
      p_user_id: a.user.id,
      p_kind: "spend_cap_warning",
      p_period: "sqa",
      p_title: "t",
      p_body: "b",
    });
    const { client: cb } = await signInWithEmailPassword(b.email, b.password);
    const summary = await cb.rpc("get_usage_summary", {
      p_from: "1970-01-01",
      p_to: "9999-12-31",
    });
    const notes = await cb.rpc("list_notifications");
    const { data: aNote } = await admin
      .from("user_notifications")
      .select("id")
      .eq("user_id", a.user.id)
      .single();
    await cb.rpc("dismiss_notification", {
      p_id: (aNote as { id: string }).id,
    });
    const { data: after } = await admin
      .from("user_notifications")
      .select("dismissed_at")
      .eq("user_id", a.user.id)
      .single();
    const reversed = await cb.rpc("get_usage_summary", {
      p_from: "9999-12-31",
      p_to: "1970-01-01",
    });

    expect(summary.data).toEqual([]);
    expect(notes.data).toEqual([]);
    expect((after as { dismissed_at: string | null }).dismissed_at).toBeNull();
    expect(reversed.error).toBeNull();

    await recordOutcome("billing-abuse-03-privacy", {
      expectations: [
        "Another user's all-time usage summary and notification list are empty.",
        "Another user cannot dismiss A's notification.",
      ],
      details: { summary: summary.data, notes: notes.data },
    });
  } finally {
    await admin.from("usage_reports").delete().eq("owner_user_id", a.user.id);
    await deleteTestUser(admin, a.user.id);
    await deleteTestUser(admin, b.user.id);
  }
}, 60000);

it("stops a capped owner from restarting their workspace by reprovision or ssh", async () => {
  const u = await createTestUser();
  try {
    await grantPro(admin, u.user.id);
    const id = u.user.id;
    const call = async (action: string) => {
      const r = await handleRemoteInstanceAction(
        { action, idempotency_key: `${action}-${randomUUID()}` },
        {
          supabase: admin,
          ownerUserId: id,
          correlationId: `sqa-${randomUUID()}`,
          hasPro: () => userHasPro(admin, id),
          stub: true,
          provider: (kind) => new LocalProvider(kind),
          spendStatus: () => spendStatusFor(admin, id),
        },
      );
      return {
        status: r.status,
        json: (await r.json()) as Record<string, unknown>,
      };
    };
    expect((await call("ensure")).status).toBe(200);
    const { data: inst } = await admin
      .from("remote_instances")
      .select("id, active_placement_id, endpoint_id")
      .eq("owner_user_id", id)
      .single();
    const i = inst as {
      id: string;
      active_placement_id: string;
      endpoint_id: string;
    };

    // $5 cap, then 24h of e2b:m usage backdated (≈ $0.40/h managed) puts the owner over it.
    const { client } = await signInWithEmailPassword(u.email, u.password);
    await client.rpc("set_spend_cap", { p_cap_usd: 5 });
    await admin.from("usage_intervals").delete().eq("owner_user_id", id);
    await admin
      .from("remote_instance_placements")
      .update({ machine_type_id: "e2b:m" })
      .eq("id", i.active_placement_id);
    const fresh = await admin.rpc("set_placement_state", {
      p_placement_id: i.active_placement_id,
      p_state: "ready",
      p_at: new Date(Date.now() - 24 * 3600_000).toISOString(),
    });
    expect(fresh.error).toBeNull();
    await admin.rpc("set_placement_state", {
      p_placement_id: i.active_placement_id,
      p_state: "suspended",
    });
    const status = await spendStatusFor(admin, id);
    expect(status.state).toBe("capped");

    const reprovision = await call("reprovision");
    const relay = await authorizeRelayRequest(
      new Request(
        `https://x.test/functions/v1/remote-ssh-relay?endpoint_id=${i.endpoint_id}&key_id=${randomUUID()}`,
        {
          headers: { authorization: "Bearer jwt", upgrade: "websocket" },
        },
      ),
      {
        getUserId: async () => id,
        store: {
          getEndpoint: async () => ({
            id: i.endpoint_id,
            instance_id: i.id,
            source: "managed",
            hostname: "localhost",
            port: 2222,
          }),
          getInstance: async () => ({
            id: i.id,
            status: "suspended",
            provider_resource_id: "r",
            provider_kind: "e2b",
            active_placement_id: i.active_placement_id,
          }),
          getClientKey: async () => ({ id: "k", revoked_at: null }),
          hasPro: (o) => userHasPro(admin, o),
          spendCapped: async (o) =>
            (await spendStatusFor(admin, o)).state === "capped",
        },
      },
    ).catch((e: unknown) => e);

    expect(reprovision).toMatchObject({
      status: 402,
      json: { code: "spend_cap_reached" },
    });
    expect(relay).toBeInstanceOf(RelayError);
    expect((relay as RelayError).code).toBe("spend_cap_reached");

    await recordOutcome("billing-abuse-04-capped-restart", {
      expectations: [
        "With real 048 spend status over a $5 cap, reprovision is refused with 402 spend_cap_reached.",
        "The ssh relay refuses the capped owner with 402 spend_cap_reached instead of waking the machine.",
      ],
      details: {
        status,
        reprovision: reprovision.json,
        relayCode: (relay as RelayError).code,
      },
    });
  } finally {
    await admin
      .from("remote_instances")
      .delete()
      .eq("owner_user_id", u.user.id);
    await admin.from("spend_caps").delete().eq("user_id", u.user.id);
    await clearBilling(admin, [u.user.id]);
    await deleteTestUser(admin, u.user.id);
  }
}, 90000);

it("bounds synced config storage per user", async () => {
  const u = await createTestUser();
  try {
    const { client } = await signInWithEmailPassword(u.email, u.password);
    const results: string[] = [];
    for (let i = 0; i < 101; i++) {
      const { error } = await client.rpc("put_user_config", {
        p_scope: `repo:${randomUUID()}`,
        p_expected_version: 0,
        p_files: { "f.txt": "eA==" },
      });
      results.push(error ? error.message : "ok");
    }
    const long = await client.rpc("put_user_config", {
      p_scope: `repo:${"x".repeat(300)}`,
      p_expected_version: 0,
      p_files: {},
    });
    const { data: rows } = await admin
      .from("user_config_snapshots")
      .select("scope")
      .eq("owner_user_id", u.user.id);

    expect(results.slice(0, 100).every((r) => r === "ok")).toBe(true);
    expect(results[100]).toContain("config_scope_limit");
    expect(long.error?.message).toContain("config_scope_too_long");
    expect((rows ?? []).length).toBe(100);

    await recordOutcome("billing-abuse-05-config-storage", {
      expectations: [
        "A user can create 100 config scopes; the 101st is refused with config_scope_limit.",
        "A scope name over 256 characters is refused with config_scope_too_long.",
      ],
      details: {
        accepted: results.filter((r) => r === "ok").length,
        last: results[100],
        long: long.error?.message,
      },
    });
  } finally {
    await admin
      .from("user_config_snapshots")
      .delete()
      .eq("owner_user_id", u.user.id);
    await deleteTestUser(admin, u.user.id);
  }
}, 120000);

it("keeps cloud workspaces to personal Pro, so a Team member's usage is never unbilled", async () => {
  const u = await createTestUser();
  let orgId: string | null = null;
  try {
    const { data: org, error } = await admin
      .from("organizations")
      .insert({ name: "SQA usage team" })
      .select("id")
      .single();
    if (error) throw new Error(error.message);
    orgId = (org as { id: string }).id;
    await admin
      .from("organization_members")
      .insert({ org_id: orgId, user_id: u.user.id, role: "owner" });
    const { grantTeam } = await import("../billing");
    const { userHasCloud } =
      await import("../../../supabase/functions/_shared/billing/entitlement");
    await grantTeam(admin, orgId);
    const id = u.user.id;

    const r = await handleRemoteInstanceAction(
      { action: "ensure", idempotency_key: `ensure-${randomUUID()}` },
      {
        supabase: admin,
        ownerUserId: id,
        correlationId: `sqa-${randomUUID()}`,
        hasPro: () => userHasCloud(admin, id),
        stub: true,
        provider: (kind) => new LocalProvider(kind),
        spendStatus: () => spendStatusFor(admin, id),
      },
    );
    const body = (await r.json()) as Record<string, unknown>;
    const { data: instances } = await admin
      .from("remote_instances")
      .select("id")
      .eq("owner_user_id", id);

    expect(await userHasPro(admin, id)).toBe(true);
    expect(await userHasCloud(admin, id)).toBe(false);
    expect(r.status).toBe(402);
    expect(body.code).toBe("pro_required");
    expect(instances ?? []).toEqual([]);

    await recordOutcome("billing-abuse-06-team-member-cloud", {
      expectations: [
        "A Team-only member keeps Pro features (has_pro) but has no cloud (has_cloud false).",
        "ensure is refused with 402 pro_required and creates no instance, so no unbillable usage can start.",
      ],
      details: { status: r.status, body },
    });
  } finally {
    await admin
      .from("remote_instances")
      .delete()
      .eq("owner_user_id", u.user.id);
    if (orgId) {
      await clearBilling(admin, [orgId]);
      await admin.from("organizations").delete().eq("id", orgId);
    }
    await deleteTestUser(admin, u.user.id);
  }
}, 90000);

it("sends an account's remaining usage, then cancels with a final invoice, before deleting it", async () => {
  const u = await createTestUser();
  const id = u.user.id;
  let deleted = false;
  try {
    const grant = await grantPro(admin, id);
    const { deleteAccount, deleteAccountStoreFor } =
      await import("../../../supabase/functions/delete-account/lib");
    const { data: inst, error } = await admin
      .from("remote_instances")
      .insert({
        owner_user_id: id,
        provider_kind: "e2b",
        provider_resource_id: `sqa-${randomUUID().slice(0, 8)}`,
        region: "us_east",
        size_preset: "small",
        status: "provisioning",
        tier_id: "s",
        machine_type_id: "e2b:s",
      })
      .select("id")
      .single();
    if (error) throw new Error(error.message);
    const instanceId = (inst as { id: string }).id;
    const { data: pl } = await admin
      .from("remote_instance_placements")
      .insert({
        instance_id: instanceId,
        owner_user_id: id,
        provider_kind: "e2b",
        machine_type_id: "e2b:s",
        region: "us_east",
        provider_resource_id: "sqa-vm",
        state: "provisioning",
        role: "active",
      })
      .select("id")
      .single();
    const placementId = (pl as { id: string }).id;
    await admin
      .from("remote_instances")
      .update({ active_placement_id: placementId })
      .eq("id", instanceId);
    await admin.rpc("set_placement_state", {
      p_placement_id: placementId,
      p_state: "ready",
      p_at: new Date(Date.now() - 90 * 60_000).toISOString(),
    });

    const calls: Array<{
      method: string;
      path: string;
      params?: Record<string, unknown>;
    }> = [];
    const result = await deleteAccount({
      method: "POST",
      authorization: "Bearer jwt",
      verifyUser: async () => ({ id }),
      storeFor: (userId) => deleteAccountStoreFor(admin as never, userId),
      stripe: {
        request: async <T>(
          method: string,
          path: string,
          params?: Record<string, unknown>,
        ) => {
          calls.push({ method, path, params });
          return {
            id: grant.subscriptionId,
            status: method === "GET" ? "active" : "canceled",
          } as T;
        },
      },
      provider: (kind) => new LocalProvider(kind),
      revokeLinearToken: async () => {},
      usageReporting: { enabled: true },
    });
    deleted = result.status === 200;
    const meter = calls.filter((c) => c.path === "/v1/billing/meter_events");
    const currentHour = new Date();
    currentHour.setUTCMinutes(0, 0, 0);
    const currentId = `${id}:${currentHour.toISOString().slice(0, 13)}`;
    const meterIndex = calls.findIndex(
      (c) => c.path === "/v1/billing/meter_events",
    );
    const cancelIndex = calls.findIndex((c) => c.method === "DELETE");
    const { data: gone } = await admin.auth.admin.getUserById(id);

    expect(result.status).toBe(200);
    expect(meter.map((m) => m.params?.identifier)).toContain(currentId);
    expect(meter.length).toBeGreaterThanOrEqual(2);
    expect(meterIndex).toBeLessThan(cancelIndex);
    expect(calls[cancelIndex].params).toEqual({ invoice_now: true });
    expect(gone.user).toBeNull();

    await recordOutcome("billing-abuse-07-delete-account-usage", {
      expectations: [
        "Deleting an account sends meter events for all its unreported hours, including the current partial hour.",
        "The meter events go out before the subscription is cancelled, and the cancel asks for invoice_now.",
        "The auth user is gone afterwards.",
      ],
      details: {
        status: result.status,
        meterEvents: meter.map((m) => ({
          id: m.params?.identifier,
          value: m.params?.["payload[value]"],
        })),
        cancel: calls[cancelIndex],
      },
    });
  } finally {
    if (!deleted) {
      await admin.from("remote_instances").delete().eq("owner_user_id", id);
      await deleteTestUser(admin, id).catch(() => {});
    }
    await clearBilling(admin, [id]);
  }
}, 90000);

it("gives no second trial to an email whose account was deleted", async () => {
  const tag = randomUUID().slice(0, 8);
  // Unique local part per run; +tag, dots and googlemail must all fold away.
  const first = await createTestUser({
    email: `sqa.trial.${tag}+first@gmail.com`,
  });
  let second: Awaited<ReturnType<typeof createTestUser>> | null = null;
  try {
    const grant = await grantPro(admin, first.user.id);
    await admin
      .from("billing_customers")
      .update({ trial_used_at: new Date().toISOString() })
      .eq("stripe_customer_id", grant.customerId);
    await deleteTestUser(admin, first.user.id);
    second = await createTestUser({
      email: `sqatrial${tag}+again@googlemail.com`,
    });
    const { billingStoreFor } =
      await import("../../../supabase/functions/_shared/billing/store");
    const claimed = await billingStoreFor(
      admin as never,
      second.user.id,
    ).trialClaimed();
    const other = await createTestUser();
    const otherClaimed = await billingStoreFor(
      admin as never,
      other.user.id,
    ).trialClaimed();
    await deleteTestUser(admin, other.user.id);

    expect(claimed).toBe(true);
    expect(otherClaimed).toBe(false);

    await recordOutcome("billing-abuse-08-trial-reuse", {
      expectations: [
        "After an account that used its trial is deleted, a new account with the same Gmail address (other +tag, googlemail, no dots) has no trial.",
        "An unrelated email still has its trial.",
      ],
      details: { claimed, otherClaimed },
    });
  } finally {
    await clearBilling(admin, [first.user.id]);
    await deleteTestUser(admin, first.user.id).catch(() => {});
    if (second) await deleteTestUser(admin, second.user.id).catch(() => {});
  }
}, 90000);
