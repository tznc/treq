/**
 * Property tests for cloud metering and billing (046 usage ledger, 048 spend
 * cap) against the local database.
 *
 * Each seed drives one workspace through a random timeline of placement
 * states with real set_placement_state / cutover_placement /
 * build_usage_reports calls, including out-of-order provider timestamps,
 * managed→BYOS cutovers and stopped-disk billing. The invariants are checked
 * against an independent recomputation from usage_intervals.
 */
import { randomUUID } from "node:crypto";
import { beforeAll, expect, it } from "vitest";
import { clearBilling, grantPro } from "../billing";
import { getServiceClient } from "../clients";
import { recordOutcome } from "../record";
import { createTestUser, deleteTestUser } from "../seed";

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
const SEEDS = Number(process.env.METERING_FUZZ_SEEDS ?? 12);
const STATES = [
  "ready",
  "waking",
  "suspended",
  "degraded",
  "deleting",
  "failed",
  "bootstrapping",
  "verifying",
  "installing_access",
  "reprovisioning",
];

function prng(seed: number) {
  let s = seed >>> 0;
  return () => {
    s = (s + 0x6d2b79f5) >>> 0;
    let t = s;
    t = Math.imul(t ^ (t >>> 15), t | 1);
    t ^= t + Math.imul(t ^ (t >>> 7), t | 61);
    return ((t ^ (t >>> 14)) >>> 0) / 4294967296;
  };
}

async function rpc<T>(fn: string, args: Record<string, unknown>): Promise<T> {
  const { data, error } = await admin.rpc(fn, args);
  if (error) throw new Error(`${fn}: ${error.message}`);
  return data as T;
}

async function rows<T>(
  q: PromiseLike<{ data: unknown; error: { message: string } | null }>,
): Promise<T[]> {
  const { data, error } = await q;
  if (error) throw new Error(error.message);
  return (data ?? []) as T[];
}

type Interval = {
  placement_id: string;
  kind: "running" | "stopped_storage";
  price_id: string;
  disk_gb: number | null;
  started_at: string;
  ended_at: string | null;
  billing_mode: "managed" | "byos";
};
type Price = {
  id: string;
  running_nanousd_per_sec: number;
  stopped_nanousd_per_gb_sec: number;
  markup_bps: number;
  machine_type_id: string;
};
type ListPrice = {
  tier_id: string;
  managed_usd_per_hour: number;
  byos_usd_per_hour: number;
  stopped_managed_usd_per_gb_month: number;
  stopped_byos_usd_per_gb_month: number;
};

/** Billed nano-USD for `sec` seconds of an interval at its size's list price (072). */
function billedFor(
  i: { kind: string; billing_mode: string; disk_gb: number | null },
  l: ListPrice,
  sec: number,
): number {
  const byos = i.billing_mode === "byos";
  if (i.kind === "running") {
    return (
      (sec *
        Number(byos ? l.byos_usd_per_hour : l.managed_usd_per_hour) *
        1e9) /
      3600
    );
  }
  const perGbMonth = Number(
    byos ? l.stopped_byos_usd_per_gb_month : l.stopped_managed_usd_per_gb_month,
  );
  return (sec * perGbMonth * 1e9 * (i.disk_gb ?? 0)) / (730 * 3600);
}

const ms = (s: string) => new Date(s).getTime();
const HOUR = 3_600_000;

async function placement(
  instanceId: string,
  ownerId: string,
  mt: string,
  kind: string,
  role: string,
  credentialId: string | null,
) {
  const id = randomUUID();
  const { error } = await admin.from("remote_instance_placements").insert({
    id,
    instance_id: instanceId,
    owner_user_id: ownerId,
    provider_kind: kind,
    machine_type_id: mt,
    region: "us_east",
    provider_resource_id: `fuzz-${id.slice(0, 8)}`,
    state: "provisioning",
    role,
    credential_source: credentialId ? "byos" : "managed",
    credential_id: credentialId,
  });
  if (error) throw new Error(`placement: ${error.message}`);
  return id;
}

it(`keeps the usage ledger and reports consistent over ${SEEDS} random timelines`, async () => {
  const failures: string[] = [];
  const summary: Array<Record<string, unknown>> = [];
  const users: string[] = [];
  try {
    for (let seed = 1; seed <= SEEDS; seed++) {
      const rand = prng(seed * 7919);
      const pick = <T>(xs: T[]) => xs[Math.floor(rand() * xs.length)];
      const u = await createTestUser();
      users.push(u.user.id);
      await grantPro(admin, u.user.id);
      const owner = u.user.id;

      const instanceId = randomUUID();
      {
        const { error } = await admin.from("remote_instances").insert({
          id: instanceId,
          owner_user_id: owner,
          provider_kind: "daytona",
          provider_resource_id: "fuzz",
          region: "us_east",
          size_preset: "small",
          status: "provisioning",
          tier_id: "s",
          machine_type_id: "daytona:s",
        });
        if (error) throw new Error(error.message);
      }
      const p1 = await placement(
        instanceId,
        owner,
        "daytona:s",
        "daytona",
        "active",
        null,
      );
      await admin
        .from("remote_instances")
        .update({ active_placement_id: p1 })
        .eq("id", instanceId);

      // A monotone timeline in the past, with adversarial out-of-order stamps.
      let t = Date.now() - (3 + rand() * 3) * HOUR;
      const ops: string[] = [];
      const steps = 8 + Math.floor(rand() * 20);
      for (let i = 0; i < steps; i++) {
        t += rand() * 40 * 60_000;
        if (t > Date.now() - 10 * 60_000) break;
        const state = pick(STATES);
        const skew = rand() < 0.2 ? -(rand() * 3 * HOUR) : 0; // a stale or replayed provider timestamp
        const at = new Date(t + skew).toISOString();
        await rpc("set_placement_state", {
          p_placement_id: p1,
          p_state: state,
          p_disk_gb: rand() < 0.3 ? Math.floor(rand() * 200) : null,
          p_at: at,
        });
        ops.push(`${state}@${at}${skew ? "(skewed)" : ""}`);
      }

      // Half the seeds switch to a BYOS placement through a real cutover.
      let p2: string | null = null;
      if (rand() < 0.5) {
        const cred = await rpc<{ id: string }>("provider_credential_save", {
          p_user_id: owner,
          p_kind: "e2b",
          p_key: `k-${seed}`,
          p_label: null,
        });
        p2 = await placement(
          instanceId,
          owner,
          "e2b:s",
          "e2b",
          "candidate",
          cred.id,
        );
        await rpc("set_placement_state", {
          p_placement_id: p2,
          p_state: "ready",
          p_at: null,
        });
        const { data: inst } = await admin
          .from("remote_instances")
          .select("fence")
          .eq("id", instanceId)
          .single();
        await rpc("cutover_placement", {
          p_instance_id: instanceId,
          p_target_placement_id: p2,
          p_expected_fence: (inst as { fence: number }).fence,
        });
        // A late provider report for the new placement, stamped before the cutover.
        await rpc("set_placement_state", {
          p_placement_id: p2,
          p_state: "ready",
          p_at: new Date(Date.now() - 2 * HOUR).toISOString(),
        });
        ops.push("cutover→byos");
      }

      const placements = p2 ? [p1, p2] : [p1];
      const intervals = await rows<Interval>(
        admin
          .from("usage_intervals")
          .select(
            "placement_id, kind, price_id, disk_gb, started_at, ended_at, billing_mode",
          )
          .in("placement_id", placements)
          .order("started_at"),
      );
      const prices = new Map(
        (
          await rows<Price>(
            admin
              .from("machine_type_prices")
              .select(
                "id, running_nanousd_per_sec, stopped_nanousd_per_gb_sec, markup_bps, machine_type_id",
              )
              .in("id", [...new Set(intervals.map((i) => i.price_id))]),
          )
        ).map((p) => [p.id, p]),
      );
      const listPrices = new Map(
        (await rows<ListPrice>(admin.from("list_prices").select("*"))).map(
          (l) => [l.tier_id, l],
        ),
      );
      const tiers = new Map(
        (
          await rows<{ id: string; tier_id: string }>(
            admin.from("provider_machine_types").select("id, tier_id"),
          )
        ).map((m) => [m.id, m.tier_id]),
      );
      const listFor = (priceId: string) =>
        listPrices.get(tiers.get(prices.get(priceId)!.machine_type_id)!)!;
      const fail = (msg: string) => failures.push(`seed ${seed}: ${msg}`);

      // I1: one machine is billed at a time for a workspace.
      const now = Date.now();
      const spans = intervals.map((i) => ({
        ...i,
        s: ms(i.started_at),
        e: i.ended_at ? ms(i.ended_at) : now,
      }));
      for (let a = 0; a < spans.length; a++) {
        for (let b = a + 1; b < spans.length; b++) {
          const x = spans[a],
            y = spans[b];
          if (
            x.s < y.e &&
            y.s < x.e &&
            Math.min(x.e, y.e) - Math.max(x.s, y.s) > 1
          ) {
            fail(
              `overlapping billed intervals ${x.placement_id.slice(0, 8)}/${x.kind} ${x.started_at}..${x.ended_at} and ${y.placement_id.slice(0, 8)}/${y.kind} ${y.started_at}..${y.ended_at}`,
            );
          }
        }
      }
      // I2: an interval's billing mode is its placement's.
      for (const i of intervals) {
        const want = i.placement_id === p2 ? "byos" : "managed";
        if (i.billing_mode !== want)
          fail(`interval on ${want} placement billed as ${i.billing_mode}`);
      }

      // I3: reports are idempotent and match an independent recomputation.
      await rpc("build_usage_reports", { p_until: new Date().toISOString() });
      const again = await rpc<number>("build_usage_reports", {
        p_until: new Date().toISOString(),
      });
      if (again !== 0) fail(`a second build wrote ${again} rows`);
      const reports = await rows<{
        hour: string;
        seconds: number;
        provider_cost_nanousd: number;
        billed_nanousd: number;
        byos_billed_nanousd: number;
      }>(
        admin
          .from("usage_reports")
          .select(
            "hour, seconds, provider_cost_nanousd, billed_nanousd, byos_billed_nanousd",
          )
          .eq("owner_user_id", owner),
      );
      for (const r of reports) {
        const h0 = ms(r.hour),
          h1 = h0 + HOUR;
        let secs = 0,
          cost = 0,
          billed = 0,
          byos = 0;
        for (const i of spans) {
          const s = Math.max(i.s, h0),
            e = Math.min(i.e, h1);
          if (e <= s) continue;
          const p = prices.get(i.price_id)!;
          const sec = (e - s) / 1000;
          const c =
            sec *
            (i.kind === "running"
              ? p.running_nanousd_per_sec
              : p.stopped_nanousd_per_gb_sec * (i.disk_gb ?? 0));
          const b = billedFor(i, listFor(i.price_id), sec);
          secs += sec;
          cost += c;
          billed += b;
          if (i.billing_mode === "byos") byos += b;
        }
        const close = (a: number, b: number) =>
          Math.abs(a - b) <= 2 + Math.abs(b) * 1e-9;
        if (r.seconds > 3600)
          fail(`hour ${r.hour} reports ${r.seconds}s (> 3600)`);
        if (!close(r.seconds, Math.round(secs)))
          fail(
            `hour ${r.hour} seconds ${r.seconds} vs ledger ${secs.toFixed(1)}`,
          );
        if (!close(r.provider_cost_nanousd, cost))
          fail(
            `hour ${r.hour} cost ${r.provider_cost_nanousd} vs ${cost.toFixed(0)}`,
          );
        if (!close(r.billed_nanousd, billed))
          fail(
            `hour ${r.hour} billed ${r.billed_nanousd} vs ${billed.toFixed(0)}`,
          );
        if (!close(r.byos_billed_nanousd, byos))
          fail(
            `hour ${r.hour} byos ${r.byos_billed_nanousd} vs ${byos.toFixed(0)}`,
          );
      }

      // I4: spend = reported hours in the window + unreported ledger since.
      const spend = await rpc<{ spent_usd: number; month_start: string }>(
        "get_spend_status_for",
        { p_user_id: owner },
      );
      const w0 = ms(spend.month_start),
        tNow = Date.now();
      let ledger = 0;
      for (const i of spans) {
        const s = Math.max(i.s, w0),
          e = Math.min(i.e, tNow);
        if (e <= s) continue;
        ledger += billedFor(i, listFor(i.price_id), (e - s) / 1000);
      }
      // The open part is priced at the moment of the call, a few ms after ours.
      if (Math.abs(spend.spent_usd - ledger / 1e9) > 1e-4) {
        fail(
          `spend ${spend.spent_usd} vs ledger ${(ledger / 1e9).toFixed(6)} (window ${spend.month_start})`,
        );
      }
      summary.push({
        seed,
        ops: ops.length,
        intervals: intervals.length,
        reports: reports.length,
        byos: Boolean(p2),
        spent_usd: spend.spent_usd,
      });
    }
  } finally {
    for (const id of users) {
      await admin.from("remote_instances").delete().eq("owner_user_id", id);
    }
    await clearBilling(admin, users);
    for (const id of users) await deleteTestUser(admin, id);
  }

  await recordOutcome("metering-fuzz-01-ledger-invariants", {
    expectations: [
      "No two billed intervals of one workspace overlap, and each carries its placement's billing mode.",
      "Building reports twice writes nothing new, and each owner-hour matches an independent recomputation (≤3600s, 20% managed, 5% BYOS).",
      "Spend status equals reported hours plus unreported ledger in the window.",
    ],
    details: { failures, summary },
  });
  expect(failures).toEqual([]);
}, 600_000);
