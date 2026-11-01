// Unit tests for the idle controller (remote-admin `enforce_idle`): an
// in-memory store and a scripted provider. No network.
//
// Run with: `deno test --allow-env supabase/functions/tests/idle_controller.test.ts`

import { assertEquals } from "https://deno.land/std@0.224.0/assert/mod.ts";
import type { ActivityPin } from "../_shared/remote/activity-probe.ts";
import {
  type ActivityRecord,
  enforceIdle,
  type IdleCandidate,
  type IdleStore,
  idleTimeoutMinutes,
} from "../_shared/remote/idle-controller.ts";
import type { ManagedInstanceState, MachineExecResult } from "../_shared/remote/provider.ts";
import { providerInstance } from "./remote_fakes.ts";

const NOW = new Date("2026-10-28T12:00:00Z");
const NOW_SEC = NOW.getTime() / 1000;
const minutesAgo = (m: number) => new Date(NOW.getTime() - m * 60_000).toISOString();

function probe(opts: { ssh?: boolean; agentLogAgeSec?: number; orphanCpu?: number } = {}): string {
  const ps: unknown[] = [[1, 0, 0, 0, "?", "/sbin/init"], [900, 1, 1000, 0, "?", "bash -c probe"]];
  if (opts.ssh) ps.push([111, 1, 1000, 0, "?", "sshd: treq@notty"]);
  if (opts.orphanCpu !== undefined) ps.push([500, 1, 1000, opts.orphanCpu, "?", "node server.js"]);
  const agents: unknown[] = [];
  if (opts.agentLogAgeSec !== undefined) {
    ps.push([400, 1, 1000, 1, "?", "claude"]);
    agents.push([400, NOW_SEC - opts.agentLogAgeSec]);
  }
  return JSON.stringify({ v: 1, now: NOW_SEC, self: 900, ps, agents, services: [], ttys: [], listen: [] });
}

function candidate(overrides: Partial<IdleCandidate> = {}): IdleCandidate {
  return {
    instance_id: "inst-1",
    owner_user_id: "owner-1",
    placement_id: "pl-1",
    provider_kind: "boat",
    provider_resource_id: "bx_1",
    state: "ready",
    last_state: "running",
    last_active_at: minutesAgo(5),
    probe_failures: 0,
    migration_open: false,
    ...overrides,
  };
}

class FakeStore implements IdleStore {
  records: ActivityRecord[] = [];
  states: Array<[string, string]> = [];
  ats: Array<string | undefined> = [];
  pinRows: ActivityPin[] = [];
  constructor(public rows: IdleCandidate[]) {}
  candidates() {
    return Promise.resolve(this.rows);
  }
  pins(_instanceId: string) {
    return Promise.resolve(this.pinRows);
  }
  setPlacementState(placementId: string, state: string, at?: string) {
    this.states.push([placementId, state]);
    this.ats.push(at);
    return Promise.resolve();
  }
  recordActivity(record: ActivityRecord) {
    this.records.push(record);
    return Promise.resolve();
  }
}

class FakeProvider {
  calls: string[] = [];
  stateChangedAt: string | undefined;
  constructor(
    public observed: ManagedInstanceState = "ready",
    public exec: () => MachineExecResult | Promise<MachineExecResult> = () => ({ exitCode: 0, stdout: probe(), stderr: "" }),
  ) {}
  getInstance(id: string) {
    this.calls.push(`get:${id}`);
    return Promise.resolve(providerInstance(id, this.observed, { stateChangedAt: this.stateChangedAt }));
  }
  async execOnMachine(id: string) {
    this.calls.push(`exec:${id}`);
    return await this.exec();
  }
  stopInstance(id: string) {
    this.calls.push(`stop:${id}`);
    return Promise.resolve();
  }
  keepAlive(id: string) {
    this.calls.push(`keepalive:${id}`);
    return Promise.resolve();
  }
}

function run(store: FakeStore, provider: FakeProvider, extra: { maxProbeFailures?: number } = {}) {
  return enforceIdle({
    store,
    providers: () => provider,
    now: () => NOW,
    log: () => {},
    idleTimeoutMin: 15,
    maxProbeFailures: extra.maxProbeFailures ?? 3,
  });
}

Deno.test("TREQ_IDLE_TIMEOUT_MIN defaults to 15 minutes", () => {
  assertEquals(idleTimeoutMinutes(() => undefined), 15);
  assertEquals(idleTimeoutMinutes(() => "30"), 30);
  assertEquals(idleTimeoutMinutes(() => "-1"), 15);
});

Deno.test("an active workspace records last_active_at = now and pushes the provider auto-stop out", async () => {
  const store = new FakeStore([candidate({ last_active_at: minutesAgo(40) })]);
  const provider = new FakeProvider("ready", () => ({ exitCode: 0, stdout: probe({ ssh: true }), stderr: "" }));
  const result = await run(store, provider);
  assertEquals(result.stopped, []);
  assertEquals(provider.calls, ["get:bx_1", "exec:bx_1", "keepalive:bx_1"]);
  const [rec] = store.records;
  assertEquals(rec.last_active_at, NOW.toISOString());
  assertEquals(rec.probe_ok, true);
  assertEquals(rec.probe_failures, 0);
  assertEquals(rec.last_state, "running");
  assertEquals(rec.idle_timeout_min, 15);
  assertEquals(rec.summary?.ssh_sessions, 1);
});

Deno.test("last_active_at follows the agent log's age, not the probe time", async () => {
  const store = new FakeStore([candidate({ last_active_at: minutesAgo(30) })]);
  const provider = new FakeProvider("ready", () => ({ exitCode: 0, stdout: probe({ agentLogAgeSec: 120 }), stderr: "" }));
  await run(store, provider);
  assertEquals(store.records[0].last_active_at, minutesAgo(2));
});

Deno.test("a workspace idle for the timeout is stopped and billing closes", async () => {
  const store = new FakeStore([candidate({ last_active_at: minutesAgo(16) })]);
  const provider = new FakeProvider("ready", () => ({ exitCode: 0, stdout: probe({ orphanCpu: 90 }), stderr: "" }));
  const result = await run(store, provider);
  assertEquals(result.stopped, ["inst-1"]);
  assertEquals(provider.calls, ["get:bx_1", "exec:bx_1", "stop:bx_1"]);
  assertEquals(store.states, [["pl-1", "suspended"]]);
  assertEquals(store.records[0].last_state, "stopped");
  assertEquals(store.records[0].stopped_at, NOW.toISOString());
  // The busy orphan is reported but did not keep it awake.
  assertEquals(store.records[0].summary?.processes[0].pid, 500);
});

Deno.test("idle shorter than the timeout keeps the workspace running", async () => {
  const store = new FakeStore([candidate({ last_active_at: minutesAgo(14) })]);
  const provider = new FakeProvider();
  const result = await run(store, provider);
  assertEquals(result.stopped, []);
  assertEquals(store.records[0].last_active_at, minutesAgo(14));
  assertEquals(provider.calls.includes("keepalive:bx_1"), false);
});

Deno.test("a live pin keeps an otherwise idle workspace awake", async () => {
  const store = new FakeStore([candidate({ last_active_at: minutesAgo(60) })]);
  store.pinRows = [{ id: "pin-1", pid: null, command: null, pattern: "server.js", expires_at: minutesAgo(-60) }];
  const provider = new FakeProvider("ready", () => ({ exitCode: 0, stdout: probe({ orphanCpu: 1 }), stderr: "" }));
  const result = await run(store, provider);
  assertEquals(result.stopped, []);
  assertEquals(store.records[0].last_active_at, NOW.toISOString());
  assertEquals(store.records[0].summary?.processes[0].pinned, true);
});

Deno.test("never stops during an open migration", async () => {
  const store = new FakeStore([candidate({ last_active_at: minutesAgo(600), migration_open: true })]);
  const provider = new FakeProvider();
  const result = await run(store, provider);
  assertEquals(result.stopped, []);
  assertEquals(result.skipped, ["inst-1"]);
  assertEquals(provider.calls, []);
});

Deno.test("a migration only exempts the workspace while a target is being built or cut over", async () => {
  for (const state of ["target_provisioning", "target_bootstrapped", "verifying", "cutover"]) {
    const store = new FakeStore([candidate({ last_active_at: minutesAgo(600), migration_open: true, migration_state: state })]);
    const result = await run(store, new FakeProvider());
    assertEquals(result.skipped, ["inst-1"], state);
  }
  for (const state of ["requested", "awaiting_ack", "source_retiring", "preflight", "rolling_back"]) {
    const store = new FakeStore([candidate({ last_active_at: minutesAgo(600), migration_open: true, migration_state: state })]);
    const result = await run(store, new FakeProvider());
    assertEquals(result.skipped, [], state);
    assertEquals(result.stopped, ["inst-1"], state);
  }
});

Deno.test("probe failures do not count as activity but stop only after N in a row", async () => {
  const failing = () => ({ exitCode: 127, stdout: "", stderr: "bash: not found" });
  const store = new FakeStore([candidate({ last_active_at: minutesAgo(60), probe_failures: 1 })]);
  let result = await run(store, new FakeProvider("ready", failing));
  assertEquals(result.stopped, []);
  assertEquals(store.records[0].probe_ok, false);
  assertEquals(store.records[0].probe_failures, 2);
  assertEquals(store.records[0].last_active_at, minutesAgo(60));
  assertEquals(store.records[0].summary, undefined);

  const third = new FakeStore([candidate({ last_active_at: minutesAgo(60), probe_failures: 2 })]);
  const provider = new FakeProvider("ready", () => Promise.reject(new Error("exec timed out")));
  result = await run(third, provider);
  assertEquals(result.stopped, ["inst-1"]);
  assertEquals(third.records[0].probe_failures, 3);
});

Deno.test("the idle clock restarts when a stopped workspace is seen running again", async () => {
  const store = new FakeStore([candidate({ last_state: "stopped", last_active_at: minutesAgo(600) })]);
  const result = await run(store, new FakeProvider());
  assertEquals(result.stopped, []);
  assertEquals(store.records[0].last_active_at, NOW.toISOString());
  assertEquals(store.records[0].last_state, "running");
});

Deno.test("the first probe of a workspace starts its idle clock", async () => {
  const store = new FakeStore([candidate({ last_active_at: null })]);
  await run(store, new FakeProvider());
  assertEquals(store.records[0].last_active_at, NOW.toISOString());
});

Deno.test("reconcile: a provider auto-stop is recorded as suspended, closing billing", async () => {
  const store = new FakeStore([candidate()]);
  const provider = new FakeProvider("suspended");
  const result = await run(store, provider);
  assertEquals(result.reconciled, [{ instance_id: "inst-1", from: "ready", to: "suspended" }]);
  assertEquals(store.states, [["pl-1", "suspended"]]);
  assertEquals(provider.calls, ["get:bx_1"]);
  assertEquals(store.records[0].last_state, "stopped");
});

Deno.test("reconcile: a machine running while we think it is stopped opens billing and is probed", async () => {
  const store = new FakeStore([candidate({ state: "suspended", last_state: "stopped" })]);
  const provider = new FakeProvider("ready");
  const result = await run(store, provider);
  assertEquals(result.reconciled, [{ instance_id: "inst-1", from: "suspended", to: "ready" }]);
  assertEquals(store.states, [["pl-1", "ready"]]);
  assertEquals(provider.calls, ["get:bx_1", "exec:bx_1"]);
});

Deno.test("reconcile: a provider stop is billed until the provider's stop time", async () => {
  const store = new FakeStore([candidate({ probed_at: minutesAgo(5) })]);
  const provider = new FakeProvider("suspended");
  provider.stateChangedAt = minutesAgo(14);
  await run(store, provider);
  assertEquals(store.ats, [minutesAgo(14)]);
});

Deno.test("reconcile: without a provider time a stop is closed at the last probe", async () => {
  const store = new FakeStore([candidate({ probed_at: minutesAgo(5) })]);
  await run(store, new FakeProvider("suspended"));
  assertEquals(store.ats, [minutesAgo(5)]);
});

Deno.test("reconcile: an out-of-band start is billed from the provider's start time", async () => {
  const store = new FakeStore([candidate({ state: "suspended", last_state: "stopped", probed_at: minutesAgo(5) })]);
  const provider = new FakeProvider("ready");
  provider.stateChangedAt = minutesAgo(4);
  await run(store, provider);
  assertEquals(store.ats[0], minutesAgo(4));
});

Deno.test("reconcile: an out-of-band start with no provider time is billed from now", async () => {
  const store = new FakeStore([candidate({ state: "suspended", last_state: "stopped", probed_at: minutesAgo(5) })]);
  await run(store, new FakeProvider("ready"));
  assertEquals(store.ats[0], undefined);
});

Deno.test("a suspended placement that is still suspended is left alone", async () => {
  const store = new FakeStore([candidate({ state: "suspended", last_state: "stopped" })]);
  const provider = new FakeProvider("suspended");
  const result = await run(store, provider);
  assertEquals(result.reconciled, []);
  assertEquals(store.states, []);
  assertEquals(store.records, []);
});

Deno.test("a provider read failure is reported and changes nothing", async () => {
  const store = new FakeStore([candidate({ last_active_at: minutesAgo(600) })]);
  const provider = new FakeProvider();
  provider.getInstance = () => Promise.reject(new Error("boom"));
  const result = await run(store, provider);
  assertEquals(result.failed, [{ instanceId: "inst-1", error: "boom" }]);
  assertEquals(store.states, []);
  assertEquals(result.stopped, []);
});
