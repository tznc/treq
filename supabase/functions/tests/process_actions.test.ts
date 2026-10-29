// Unit tests for the remote-instance process actions (pin_process,
// unpin_process, kill_process): request validation and the kill check.
// No network.
//
// Run with: `deno test supabase/functions/tests/process_actions.test.ts`

import { assert, assertEquals, assertStringIncludes } from "https://deno.land/std@0.224.0/assert/mod.ts";
import type { ProbeSummary } from "../_shared/remote/activity-probe.ts";
import {
  checkKill,
  killCommand,
  killExitError,
  PIN_BILLING_NOTE,
  parseKillRequest,
  parsePinRequest,
  parseUnpinRequest,
} from "../_shared/remote/process-actions.ts";

Deno.test("pin by pid or by pattern, with a default and a capped ttl", () => {
  assertEquals(parsePinRequest({ instance_id: "i", pid: 500 }), {
    ok: true,
    value: { instanceId: "i", pid: 500, pattern: null, ttlHours: 8 },
  });
  assertEquals(parsePinRequest({ instance_id: "i", pattern: "npm run dev", ttl_hours: 24 }), {
    ok: true,
    value: { instanceId: "i", pid: null, pattern: "npm run dev", ttlHours: 24 },
  });
});

Deno.test("bad pin requests are rejected with a reason", () => {
  const cases: Array<[Record<string, unknown>, string]> = [
    [{ pid: 5 }, "instance_id is required"],
    [{ instance_id: "i" }, "exactly one of pid or pattern is required"],
    [{ instance_id: "i", pid: 5, pattern: "x" }, "exactly one of pid or pattern is required"],
    [{ instance_id: "i", pid: 1 }, "pid must be an integer above 1"],
    [{ instance_id: "i", pid: "5" }, "pid must be an integer above 1"],
    [{ instance_id: "i", pattern: "" }, "pattern must be 1 to 200 characters"],
    [{ instance_id: "i", pattern: "x".repeat(201) }, "pattern must be 1 to 200 characters"],
    [{ instance_id: "i", pid: 5, ttl_hours: 0 }, "ttl_hours must be an integer from 1 to 72"],
    [{ instance_id: "i", pid: 5, ttl_hours: 73 }, "ttl_hours must be an integer from 1 to 72"],
  ];
  for (const [body, error] of cases) {
    assertEquals(parsePinRequest(body), { ok: false, error }, JSON.stringify(body));
  }
});

Deno.test("unpin and kill requests", () => {
  assertEquals(parseUnpinRequest({ instance_id: "i", pin_id: "p" }), { ok: true, value: { instanceId: "i", pinId: "p" } });
  assertEquals(parseUnpinRequest({ instance_id: "i" }), { ok: false, error: "pin_id is required" });
  assertEquals(parseKillRequest({ instance_id: "i", pid: 42 }), { ok: true, value: { instanceId: "i", pid: 42 } });
  assertEquals(parseKillRequest({ instance_id: "i", pid: 0 }), { ok: false, error: "pid must be an integer above 1" });
});

Deno.test("the pin response tells the user pins are billed", () => {
  assertEquals(PIN_BILLING_NOTE.includes("billed"), true);
});

const summary = (processes: ProbeSummary["processes"]): ProbeSummary => ({
  ssh_sessions: 0,
  agent_turns: 0,
  pty_sessions: 0,
  active: false,
  reasons: [],
  idle_sec: null,
  live_pins: [],
  processes,
});

Deno.test("only a process from the last probe that is not a managed service can be killed", () => {
  const s = summary([
    { pid: 100, command: "sshd", origin: "service", cpu: 0, ports: [2222], pinned: false },
    { pid: 500, command: "node server.js", origin: "orphan", cpu: 3, ports: [3000], pinned: false },
  ]);
  assertEquals(checkKill(s, 500), { ok: true, command: "node server.js" });
  assertEquals(checkKill(s, 100), { ok: false, error: "managed services cannot be killed" });
  assertEquals(checkKill(s, 777), { ok: false, error: "process not found in the last probe" });
  assertEquals(checkKill(null, 500), { ok: false, error: "process not found in the last probe" });
});

Deno.test("kill checks the live cmdline against the probe's command before signalling", () => {
  const { cmd, env } = killCommand(500, "node server.js");
  assertEquals(cmd.slice(0, 2), ["/bin/sh", "-c"]);
  assertEquals(cmd.slice(3), ["sh", "500"]);
  // The expected command goes in the environment, never into the script.
  assertEquals(env, { TREQ_KILL_EXPECTED: "node server.js" });
  assert(!cmd[2].includes("node server.js"));
  const script = cmd[2];
  assertStringIncludes(script, `/proc/$pid/cmdline`);
  assertStringIncludes(script, `[ "$actual" = "$TREQ_KILL_EXPECTED" ]`);
  assert(script.indexOf("TREQ_KILL_EXPECTED") < script.indexOf("kill -TERM"), "compare before kill");
});

Deno.test("kill refuses a pid a managed service's pidfile names", () => {
  const script = killCommand(500, "x").cmd[2];
  assertStringIncludes(script, "/var/run/treq/*.pid");
  assert(script.indexOf("/var/run/treq/*.pid") < script.indexOf("kill -TERM"));
});

Deno.test("kill exit codes map to reasons", () => {
  assertEquals(killExitError(3), "managed services cannot be killed");
  assertEquals(killExitError(2), "process changed since the last probe");
  assertEquals(killExitError(1), "process is no longer running");
});
