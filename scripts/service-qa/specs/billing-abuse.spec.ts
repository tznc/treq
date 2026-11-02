/**
 * Bad actors against billing and metering, through the local stack as a
 * signed-in user (PostgREST, RLS, grants) plus the remote-instance and relay
 * handlers in-process against the real database.
 */
import { randomUUID } from "node:crypto";
import { expect, it } from "vitest";
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
          c
            .from("usage_intervals")
            .insert({
              placement_id: randomUUID(),
              owner_user_id: id,
              kind: "running",
              price_id: randomUUID(),
            }),
        ],
        [
          "usage_reports.insert",
          c
            .from("usage_reports")
            .insert({
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
          c
            .from("billing_subscriptions")
            .insert({
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

// Open: put_user_config has no per-user scope limit (docs/qa/byos-chaos-2026-11-01.md).
// it.fails keeps the repro running; change to it() once scopes are bounded.
it.fails(
  "bounds synced config storage per user",
  async () => {
    const u = await createTestUser();
    try {
      const { client } = await signInWithEmailPassword(u.email, u.password);
      const blob = { "f.txt": "x".repeat(200_000) };
      const results: string[] = [];
      for (let i = 0; i < 40; i++) {
        const { error } = await client.rpc("put_user_config", {
          p_scope: `repo:${randomUUID()}`,
          p_expected_version: 0,
          p_files: blob,
        });
        results.push(error ? error.message : "ok");
      }
      const { data: size } = await admin
        .from("user_config_snapshots")
        .select("scope")
        .eq("owner_user_id", u.user.id);
      const stored = (size ?? []).length;

      await recordOutcome("billing-abuse-05-config-storage", {
        expectations: [
          "A Free user cannot create an unbounded number of config scopes (each up to 1 MiB × 20 versions).",
        ],
        details: {
          accepted: results.filter((r) => r === "ok").length,
          stored,
          bytesPerScope: 200_000,
        },
      });
      // Documents the current bound; fails while scopes are unlimited.
      expect(stored).toBeLessThan(40);
    } finally {
      await admin
        .from("user_config_snapshots")
        .delete()
        .eq("owner_user_id", u.user.id);
      await deleteTestUser(admin, u.user.id);
    }
  },
  120000,
);

// Open: a Team member passes the Pro gate but has no personal Stripe customer,
// so their usage ends as no_customer. it.fails keeps the repro; flip to it() when fixed.
it.fails(
  "bills a Team member's cloud usage somewhere",
  async () => {
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
      await grantTeam(admin, orgId);
      const id = u.user.id;
      expect(await userHasPro(admin, id)).toBe(true);

      const r = await handleRemoteInstanceAction(
        { action: "ensure", idempotency_key: `ensure-${randomUUID()}` },
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
      expect(r.status).toBe(200);
      const { data: inst } = await admin
        .from("remote_instances")
        .select("active_placement_id")
        .eq("owner_user_id", id)
        .single();
      const pid = (inst as { active_placement_id: string }).active_placement_id;
      await admin.from("usage_intervals").delete().eq("owner_user_id", id);
      await admin
        .from("remote_instance_placements")
        .update({ machine_type_id: "e2b:m" })
        .eq("id", pid);
      await admin.rpc("set_placement_state", {
        p_placement_id: pid,
        p_state: "ready",
        p_at: new Date(Date.now() - 3 * 3600_000).toISOString(),
      });
      await admin.rpc("set_placement_state", {
        p_placement_id: pid,
        p_state: "suspended",
      });

      const { reportUsage, supabaseUsageReportStore } =
        await import("../../../supabase/functions/_shared/billing/usage-report");
      const sent: unknown[] = [];
      const result = await reportUsage({
        store: supabaseUsageReportStore(admin),
        stripe: {
          request: async (_m: string, _p: string, params?: unknown) => {
            sent.push(params);
            return {} as never;
          },
        },
        enabled: true,
        now: () => new Date(),
        log: () => {},
      } as never);
      const { data: rows } = await admin
        .from("usage_reports")
        .select("billed_nanousd, stripe_status")
        .eq("owner_user_id", id);
      const billed = (rows ?? []) as Array<{
        billed_nanousd: number;
        stripe_status: string;
      }>;
      const unbilledUsd =
        billed
          .filter((x) => x.stripe_status === "no_customer")
          .reduce((a, x) => a + Number(x.billed_nanousd), 0) / 1e9;

      await recordOutcome("billing-abuse-06-team-member-usage", {
        expectations: [
          "A Team member who passes the Pro gate has their cloud usage reported to some Stripe customer, not dropped as no_customer.",
        ],
        details: {
          reportRows: billed,
          unbilledUsd,
          sentEvents: sent.length,
          result,
        },
      });
      expect(unbilledUsd).toBe(0);
    } finally {
      await admin.from("usage_reports").delete().eq("owner_user_id", u.user.id);
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
  },
  90000,
);
