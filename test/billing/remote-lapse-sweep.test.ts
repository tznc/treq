import { describe, expect, it, vi } from "vitest";
import {
  type LapseCandidate,
  type LapseSweepStore,
  sweepLapsedInstances,
} from "../../supabase/functions/_shared/remote/lapse-sweep.ts";

const NOW = new Date("2026-10-05T12:00:00Z");
const DAY = 86_400_000;

function daysAgo(days: number): string {
  return new Date(NOW.getTime() - days * DAY).toISOString();
}

function candidate(overrides: Partial<LapseCandidate> = {}): LapseCandidate {
  return {
    instance_id: "inst-1",
    owner_user_id: "user-1",
    status: "ready",
    provider_resource_id: "dev-treq-user-1",
    lapsed_at: null,
    owner_has_pro: false,
    ...overrides,
  };
}

// The database side, as remote_sync_instance_lapse and
// remote_claim_lapsed_deletion behave: each re-checks entitlement and its own
// clock when it writes.
function fakeStore(
  rows: LapseCandidate[],
  db: {
    hasPro?: (owner: string) => boolean;
    claimAllowed?: (row: LapseCandidate) => boolean;
  } = {},
) {
  const hasPro =
    db.hasPro ??
    ((owner) => rows.find((r) => r.owner_user_id === owner)!.owner_has_pro);
  const byId = (id: string) => rows.find((r) => r.instance_id === id)!;
  const audits: Array<{ eventType: string; instanceId: string }> = [];
  const store: LapseSweepStore = {
    candidates: vi.fn(async () => rows.map((r) => ({ ...r }))),
    syncLapse: vi.fn(async (id: string) => {
      const row = byId(id);
      row.lapsed_at = hasPro(row.owner_user_id)
        ? null
        : (row.lapsed_at ?? NOW.toISOString());
      return row.lapsed_at;
    }),
    claimDeletion: vi.fn(async (id: string) => {
      const row = byId(id);
      const allowed = db.claimAllowed
        ? db.claimAllowed(row)
        : !hasPro(row.owner_user_id) &&
          row.lapsed_at !== null &&
          NOW.getTime() - Date.parse(row.lapsed_at) >= 30 * DAY;
      if (allowed) row.status = "deleting";
      return allowed;
    }),
    markStopped: vi.fn(async (id: string) => {
      const row = byId(id);
      if (row.status === "ready" || row.status === "waking")
        row.status = "suspended";
    }),
    markDeleted: vi.fn(async (id: string) => {
      byId(id).status = "deleted";
    }),
    audit: vi.fn(async (event) => {
      audits.push({ eventType: event.eventType, instanceId: event.instanceId });
    }),
  };
  return { store, audits };
}

function fakeProvider() {
  return {
    stopInstance: vi.fn(async (_id: string) => {}),
    deleteInstance: vi.fn(async (_id: string) => {}),
  };
}

function sweep(store: LapseSweepStore, provider = fakeProvider()) {
  return {
    provider,
    run: () =>
      sweepLapsedInstances({ store, provider, now: () => NOW, log: vi.fn() }),
  };
}

describe("sweepLapsedInstances", () => {
  it("stops the instance of an owner whose Pro just ended and records the lapse", async () => {
    const rows = [candidate()];
    const { store, audits } = fakeStore(rows);
    const { provider, run } = sweep(store);

    const result = await run();

    expect(result.stopped).toEqual(["inst-1"]);
    expect(provider.stopInstance).toHaveBeenCalledWith("dev-treq-user-1");
    expect(provider.deleteInstance).not.toHaveBeenCalled();
    expect(rows[0].lapsed_at).toBe(NOW.toISOString());
    expect(rows[0].status).toBe("suspended");
    expect(audits).toEqual([
      { eventType: "instance_lapse_stopped", instanceId: "inst-1" },
    ]);
  });

  it("keeps a stopped instance during the 30 days", async () => {
    const rows = [candidate({ status: "suspended", lapsed_at: daysAgo(10) })];
    const { store } = fakeStore(rows);
    const { provider, run } = sweep(store);

    const result = await run();

    expect(result.kept).toEqual(["inst-1"]);
    expect(store.claimDeletion).not.toHaveBeenCalled();
    expect(provider.stopInstance).not.toHaveBeenCalled();
    expect(provider.deleteInstance).not.toHaveBeenCalled();
  });

  it("stops an instance again if something woke it during the 30 days", async () => {
    const rows = [candidate({ status: "ready", lapsed_at: daysAgo(10) })];
    const { store } = fakeStore(rows);
    const { provider, run } = sweep(store);

    const result = await run();

    expect(result.stopped).toEqual(["inst-1"]);
    expect(provider.stopInstance).toHaveBeenCalledTimes(1);
    expect(rows[0].lapsed_at).toBe(daysAgo(10));
  });

  it("deletes the instance after 30 days without Pro", async () => {
    const rows = [candidate({ status: "suspended", lapsed_at: daysAgo(31) })];
    const { store, audits } = fakeStore(rows);
    const { provider, run } = sweep(store);

    const result = await run();

    expect(result.deleted).toEqual(["inst-1"]);
    expect(store.claimDeletion).toHaveBeenCalledWith("inst-1");
    expect(provider.deleteInstance).toHaveBeenCalledWith("dev-treq-user-1");
    expect(rows[0].status).toBe("deleted");
    expect(audits).toEqual([
      { eventType: "instance_lapse_deleted", instanceId: "inst-1" },
    ]);
  });

  it("never deletes when the database refuses the claim, whatever this clock says", async () => {
    // The owner subscribed again after the candidates were listed, or the
    // database clock disagrees: remote_claim_lapsed_deletion has the last word.
    const rows = [candidate({ status: "suspended", lapsed_at: daysAgo(45) })];
    const { store } = fakeStore(rows, { claimAllowed: () => false });
    const { provider, run } = sweep(store);

    const result = await run();

    expect(result.kept).toEqual(["inst-1"]);
    expect(provider.deleteInstance).not.toHaveBeenCalled();
    expect(store.markDeleted).not.toHaveBeenCalled();
  });

  it("does not ask to delete before 30 days by this clock, even if the database would allow it", async () => {
    const rows = [candidate({ status: "suspended", lapsed_at: daysAgo(29) })];
    const { store } = fakeStore(rows, { claimAllowed: () => true });
    const { provider, run } = sweep(store);

    await run();

    expect(store.claimDeletion).not.toHaveBeenCalled();
    expect(provider.deleteInstance).not.toHaveBeenCalled();
  });

  it("clears the lapse when Pro is back and leaves the instance alone", async () => {
    const rows = [
      candidate({
        status: "suspended",
        lapsed_at: daysAgo(31),
        owner_has_pro: true,
      }),
    ];
    const { store, audits } = fakeStore(rows);
    const { provider, run } = sweep(store);

    const result = await run();

    expect(result.restored).toEqual(["inst-1"]);
    expect(rows[0].lapsed_at).toBeNull();
    expect(store.claimDeletion).not.toHaveBeenCalled();
    expect(provider.stopInstance).not.toHaveBeenCalled();
    expect(provider.deleteInstance).not.toHaveBeenCalled();
    expect(audits).toEqual([
      { eventType: "instance_lapse_cleared", instanceId: "inst-1" },
    ]);
  });

  it("does not stop an instance whose owner subscribed between the listing and the stop", async () => {
    const rows = [candidate()];
    const { store } = fakeStore(rows, { hasPro: () => true });
    const { provider, run } = sweep(store);

    const result = await run();

    expect(result.stopped).toEqual([]);
    expect(provider.stopInstance).not.toHaveBeenCalled();
    expect(rows[0].status).toBe("ready");
  });

  it("deletes an instance that never got a provider resource without calling the provider", async () => {
    const rows = [
      candidate({
        status: "failed",
        provider_resource_id: null,
        lapsed_at: daysAgo(31),
      }),
    ];
    const { store } = fakeStore(rows);
    const { provider, run } = sweep(store);

    const result = await run();

    expect(result.deleted).toEqual(["inst-1"]);
    expect(provider.deleteInstance).not.toHaveBeenCalled();
    expect(rows[0].status).toBe("deleted");
  });

  it("keeps sweeping other instances when one provider call fails", async () => {
    const rows = [
      candidate({
        instance_id: "inst-a",
        owner_user_id: "user-a",
        provider_resource_id: "sprite-a",
      }),
      candidate({
        instance_id: "inst-b",
        owner_user_id: "user-b",
        provider_resource_id: "sprite-b",
      }),
    ];
    const { store, audits } = fakeStore(rows);
    const provider = fakeProvider();
    provider.stopInstance.mockRejectedValueOnce(new Error("Sprites 503"));
    const { run } = sweep(store, provider);

    const result = await run();

    expect(result.failed).toEqual([
      { instanceId: "inst-a", error: "Sprites 503" },
    ]);
    expect(result.stopped).toEqual(["inst-b"]);
    expect(audits).toContainEqual({
      eventType: "instance_lapse_sweep_failed",
      instanceId: "inst-a",
    });
  });

  it("retries the provider delete of an instance a failed sweep left in deleting", async () => {
    const rows = [candidate({ status: "deleting", lapsed_at: daysAgo(32) })];
    const { store } = fakeStore(rows);
    const { provider, run } = sweep(store);

    const result = await run();

    expect(result.deleted).toEqual(["inst-1"]);
    expect(provider.deleteInstance).toHaveBeenCalledWith("dev-treq-user-1");
  });
});
