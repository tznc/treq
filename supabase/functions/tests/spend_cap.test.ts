// Unit tests for the monthly spend cap (docs/design/sandbox-provider-adapter.md §5).
// A fake store and fake providers; no network.
//
// Run with: `deno test supabase/functions/tests/spend_cap.test.ts`

import { assertEquals } from "https://deno.land/std@0.224.0/assert/mod.ts";
import {
  enforceSpendCaps,
  type RunningWorkspace,
  type SpendCapCandidate,
  type SpendCapStore,
  spendCapRefusal,
} from "../_shared/billing/spend-cap.ts";
import type { ProviderKind } from "../_shared/remote/provider.ts";

const NOW = new Date("2026-10-28T12:05:00Z");
const WARN = "00000000-0000-0000-0000-0000000000a1";
const CAPPED = "00000000-0000-0000-0000-0000000000a2";

function candidate(user_id: string, state: "warning" | "capped"): SpendCapCandidate {
  return {
    user_id,
    cap_usd: 50,
    spent_usd: state === "capped" ? 50.5 : 41,
    month_start: "2026-10-01T00:00:00+00:00",
    state,
  };
}

function fakeStore(running: Record<string, RunningWorkspace[]>, seen = new Set<string>()) {
  const notices: Array<{ userId: string; kind: string; period: string }> = [];
  const stopped: Array<{ instanceId: string; placementId: string | null }> = [];
  const rolledBackFor: string[] = [];
  const store: SpendCapStore = {
    candidates: async () => [candidate(WARN, "warning"), candidate(CAPPED, "capped")],
    notify: async (n) => {
      const key = `${n.userId}:${n.kind}:${n.period}`;
      if (seen.has(key)) return false;
      seen.add(key);
      notices.push({ userId: n.userId, kind: n.kind, period: n.period });
      return true;
    },
    runningWorkspaces: async (userId) => running[userId] ?? [],
    markStopped: async (instanceId, placementId) => {
      stopped.push({ instanceId, placementId });
    },
    rollBackOpenResizes: async (userId) => {
      rolledBackFor.push(userId);
      return userId === CAPPED ? ["mig-1"] : [];
    },
  };
  return { store, notices, stopped, seen, rolledBackFor };
}

function fakeProviders(failOn?: string) {
  const calls: Array<[ProviderKind, string]> = [];
  return {
    calls,
    providers: (kind: ProviderKind) => ({
      stopInstance: async (id: string) => {
        if (id === failOn) throw new Error("provider down");
        calls.push([kind, id]);
      },
    }),
  };
}

const ws = (id: string, kind: ProviderKind = "boat"): RunningWorkspace => ({
  instance_id: id,
  provider_kind: kind,
  provider_resource_id: `res-${id}`,
  active_placement_id: `pl-${id}`,
});

Deno.test("warns at 80% and stops every running workspace at 100%", async () => {
  const { store, notices, stopped } = fakeStore({
    [WARN]: [ws("w1")],
    [CAPPED]: [ws("c1", "daytona"), ws("c2")],
  });
  const p = fakeProviders();
  const result = await enforceSpendCaps({ store, providers: p.providers, now: () => NOW, log: () => {} });

  assertEquals(notices, [
    { userId: WARN, kind: "spend_cap_warning", period: "2026-10" },
    { userId: CAPPED, kind: "spend_cap_reached", period: "2026-10" },
  ]);
  assertEquals(p.calls, [["daytona", "res-c1"], ["boat", "res-c2"]]);
  assertEquals(stopped, [
    { instanceId: "c1", placementId: "pl-c1" },
    { instanceId: "c2", placementId: "pl-c2" },
  ]);
  assertEquals(result, {
    warned: [WARN],
    capped: [CAPPED],
    stopped: ["c1", "c2"],
    failed: [],
    rolledBackMigrations: ["mig-1"],
  });
});

Deno.test("a capped user's open resizes are rolled back, even with nothing running", async () => {
  const { store, rolledBackFor } = fakeStore({});
  const result = await enforceSpendCaps({ store, providers: fakeProviders().providers, now: () => NOW, log: () => {} });
  assertEquals(rolledBackFor, [CAPPED]);
  assertEquals(result.rolledBackMigrations, ["mig-1"]);
});

Deno.test("notices are sent once a month; stops repeat while capped", async () => {
  const seen = new Set<string>();
  const first = fakeStore({ [CAPPED]: [ws("c1")] }, seen);
  await enforceSpendCaps({ store: first.store, providers: fakeProviders().providers, now: () => NOW, log: () => {} });
  const second = fakeStore({ [CAPPED]: [ws("c1")] }, seen);
  const result = await enforceSpendCaps({
    store: second.store,
    providers: fakeProviders().providers,
    now: () => NOW,
    log: () => {},
  });
  assertEquals(second.notices, []);
  assertEquals(result.warned, []);
  assertEquals(result.capped, []);
  assertEquals(result.stopped, ["c1"], "a workspace woken past the cap is stopped again");
});

Deno.test("a provider failure on one workspace does not stop the rest", async () => {
  const { store, stopped } = fakeStore({ [CAPPED]: [ws("c1"), ws("c2")] });
  const result = await enforceSpendCaps({
    store,
    providers: fakeProviders("res-c1").providers,
    now: () => NOW,
    log: () => {},
  });
  assertEquals(result.failed, [{ instanceId: "c1", error: "provider down" }]);
  assertEquals(stopped, [{ instanceId: "c2", placementId: "pl-c2" }]);
});

Deno.test("ensure, wake and resize are refused with 402 when capped", () => {
  const status = { cap_usd: 50, spent_usd: 50.25, month_start: "2026-10-01T00:00:00+00:00", state: "capped" as const };
  for (const action of ["ensure", "wake", "resize"]) {
    assertEquals(spendCapRefusal(action, status), {
      status: 402,
      body: {
        code: "spend_cap_reached",
        error: "You reached your monthly cloud spend cap of $50.00. Raise the cap to keep working.",
        cap_usd: 50,
        spent_usd: 50.25,
      },
    });
  }
});

Deno.test("other actions and uncapped users are not refused", () => {
  const capped = { cap_usd: 50, spent_usd: 60, month_start: "", state: "capped" as const };
  assertEquals(spendCapRefusal("status", capped), null);
  assertEquals(spendCapRefusal("delete", capped), null);
  assertEquals(spendCapRefusal("ensure", { ...capped, spent_usd: 45, state: "warning" }), null);
});

Deno.test("remote-instance refuses ensure with 402 before touching the database", async () => {
  const { handleRemoteInstanceAction } = await import("../remote-instance/lib.ts");
  const res = await handleRemoteInstanceAction({ action: "ensure" }, {
    // deno-lint-ignore no-explicit-any
    // biome-ignore lint/suspicious/noExplicitAny: the cap check never touches the client
    supabase: {} as any,
    ownerUserId: CAPPED,
    correlationId: "c",
    hasPro: async () => true,
    spendStatus: async () => ({ cap_usd: 50, spent_usd: 51, month_start: "", state: "capped" }),
    stub: true,
    provider: () => {
      throw new Error("no provider");
    },
  });
  assertEquals(res.status, 402);
  const body = await res.json();
  assertEquals([body.code, body.cap_usd, body.spent_usd], ["spend_cap_reached", 50, 51]);
});

Deno.test("notices are once per Stripe billing period, not per calendar month", async () => {
  const { store, notices } = fakeStore({});
  store.candidates = async () => [{ ...candidate(CAPPED, "capped"), month_start: "2026-10-15T09:00:00+00:00" }];
  await enforceSpendCaps({ store, providers: fakeProviders().providers, now: () => NOW, log: () => {} });
  assertEquals(notices, [{ userId: CAPPED, kind: "spend_cap_reached", period: "2026-10-15T09" }]);
});
