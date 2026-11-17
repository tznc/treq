// Unit tests for the in-sandbox activity probe: the script's shape and the
// parser that turns its JSON into what keeps a workspace awake. No network.
//
// Run with: `deno test supabase/functions/tests/activity_probe.test.ts`

import { assert, assertEquals, assertThrows } from "https://deno.land/std@0.224.0/assert/mod.ts";
import {
  type ActivityPin,
  PROBE_SCRIPT,
  ProbeParseError,
  parseProbeOutput,
  probeCommand,
  summarizeProbe,
} from "../_shared/remote/activity-probe.ts";

const NOW = 1_800_000_000; // seconds
const now = new Date(NOW * 1000);

// A sandbox with: sshd (service, pidfile 100) and one ssh session, a pty
// daemon with one shell whose tty saw input 30 s ago, an agent (pid 400)
// whose log was written 20 s ago with a child, and an orphaned dev server.
function sample(overrides: Record<string, unknown> = {}): string {
  return JSON.stringify({
    v: 1,
    now: NOW,
    self: 900,
    ps: [
      [1, 0, 0, 0, "?", "/sbin/init"],
      [100, 1, 1000, 0, "?", "/usr/sbin/sshd -D -f /etc/treq/sshd_config"],
      [110, 100, 0, 0, "?", "sshd: treq [priv]"],
      [111, 110, 1000, 0.1, "?", "sshd: treq@notty"],
      [300, 1, 1000, 0.2, "?", "/usr/local/bin/treq pty-remote daemon"],
      [310, 300, 1000, 0, "pts/0", "-bash"],
      [311, 310, 1000, 1.5, "pts/0", "cargo watch -x test"],
      [400, 1, 1000, 12.5, "?", "claude -p fix the build"],
      [410, 400, 1000, 3, "?", "npm run dev"],
      [500, 1, 1000, 45.2, "?", "node server.js --port 3000"],
      [600, 1, 0, 0, "?", "/usr/sbin/cron -f"],
      [880, 1, 1000, 0, "?", "boat-exec"],
      [900, 880, 1000, 0, "?", "bash -c probe"],
      [901, 900, 1000, 0, "?", "ps -eo pid="],
    ],
    services: [100],
    agents: [[400, NOW - 20]],
    ttys: [["/dev/pts/0", NOW - 30, NOW - 40]],
    listen: [[500, 3000], [410, 5173], [100, 2222]],
    ...overrides,
  });
}

Deno.test("the probe command runs the script with bash", () => {
  assertEquals(probeCommand(), ["bash", "-c", PROBE_SCRIPT]);
  assert(PROBE_SCRIPT.includes("ps -eo"), "lists processes");
  assert(PROBE_SCRIPT.includes(".treq/agents"), "reads agent records");
  assert(PROBE_SCRIPT.includes("/var/run/treq"), "reads service pidfiles");
});

Deno.test("parse rejects output that is not a v1 probe", () => {
  assertThrows(() => parseProbeOutput("bash: ps: command not found"), ProbeParseError);
  assertThrows(() => parseProbeOutput(JSON.stringify({ v: 2, ps: [] })), ProbeParseError);
  assertThrows(() => parseProbeOutput(JSON.stringify({ v: 1, now: NOW, self: 1, ps: "x" })), ProbeParseError);
});

Deno.test("parse tolerates missing optional sections and noise lines before the JSON", () => {
  const raw = parseProbeOutput(
    "warning: something\n" + JSON.stringify({ v: 1, now: NOW, self: 9, ps: [[9, 1, 1000, 0, "?", "bash"]] }),
  );
  assertEquals(raw.services, []);
  assertEquals(raw.agents, []);
  assertEquals(raw.ttys, []);
  assertEquals(raw.listen, []);
  assertEquals(raw.ps.length, 1);
});

Deno.test("summary classifies processes by origin and hides the probe and system processes", () => {
  const s = summarizeProbe(parseProbeOutput(sample()), [], { now, recentSec: 900 });
  const byPid = new Map(s.processes.map((p) => [p.pid, p]));
  assertEquals(byPid.get(100)?.origin, "service");
  assertEquals(byPid.get(311)?.origin, "terminal");
  assertEquals(byPid.get(400)?.origin, "agent");
  assertEquals(byPid.get(410)?.origin, "agent");
  assertEquals(byPid.get(500)?.origin, "orphan");
  assertEquals(byPid.get(500)?.ports, [3000]);
  assertEquals(byPid.get(500)?.cpu, 45.2);
  assertEquals(byPid.get(410)?.ports, [5173]);
  for (const hidden of [1, 600, 880, 900, 901, 110, 111, 300]) {
    assert(!byPid.has(hidden), `pid ${hidden} is hidden`);
  }
  // Busiest first.
  assertEquals(s.processes[0].pid, 500);
});

Deno.test("ssh sessions, recent agent turns and recent terminals are activity", () => {
  const s = summarizeProbe(parseProbeOutput(sample()), [], { now, recentSec: 900 });
  assertEquals(s.ssh_sessions, 1);
  assertEquals(s.agent_turns, 1);
  assertEquals(s.pty_sessions, 1);
  assertEquals(s.active, true);
  assertEquals(s.reasons, ["ssh", "agent", "terminal"]);
  // An open ssh session is activity right now.
  assertEquals(s.idle_sec, 0);
});

Deno.test("idle_sec is the age of the freshest agent log or terminal touch", () => {
  const s = summarizeProbe(parseProbeOutput(sample({ ps: [
    [1, 0, 0, 0, "?", "/sbin/init"],
    [300, 1, 1000, 0, "?", "treq pty-remote daemon"],
    [310, 300, 1000, 0, "pts/0", "-bash"],
    [400, 1, 1000, 0, "?", "claude"],
  ] })), [], { now, recentSec: 900 });
  assertEquals(s.idle_sec, 20);
});

Deno.test("an unpinned background process alone does not keep the machine awake", () => {
  const raw = parseProbeOutput(sample({
    ps: [
      [1, 0, 0, 0, "?", "/sbin/init"],
      [500, 1, 1000, 99, "?", "node server.js --port 3000"],
      [900, 1, 1000, 0, "?", "bash -c probe"],
    ],
    agents: [],
    ttys: [],
  }));
  const s = summarizeProbe(raw, [], { now, recentSec: 900 });
  assertEquals(s.active, false);
  assertEquals(s.reasons, []);
  assertEquals(s.idle_sec, null);
  assertEquals(s.processes.map((p) => p.pid), [500]);
});

Deno.test("a stale agent log and an untouched terminal are not activity", () => {
  const raw = parseProbeOutput(sample({
    ps: [
      [1, 0, 0, 0, "?", "/sbin/init"],
      [300, 1, 1000, 0, "?", "treq pty-remote daemon"],
      [310, 300, 1000, 0, "pts/0", "-bash"],
      [400, 1, 1000, 0, "?", "claude"],
      [900, 1, 1000, 0, "?", "bash -c probe"],
    ],
    agents: [[400, NOW - 3600], [777, NOW]], // 777: record of a dead agent
    ttys: [["/dev/pts/0", NOW - 3600, NOW - 3600]],
  }));
  const s = summarizeProbe(raw, [], { now, recentSec: 900 });
  assertEquals(s.agent_turns, 0);
  assertEquals(s.pty_sessions, 0);
  assertEquals(s.active, false);
  assertEquals(s.idle_sec, 3600);
});

Deno.test("openssh 9.8 sshd-session processes count as ssh sessions", () => {
  const raw = parseProbeOutput(sample({
    ps: [[1, 0, 0, 0, "?", "/sbin/init"], [120, 1, 1000, 0, "?", "sshd-session: treq@notty"]],
    agents: [],
    ttys: [],
  }));
  assertEquals(summarizeProbe(raw, [], { now, recentSec: 900 }).ssh_sessions, 1);
});

Deno.test("a live pin keeps the machine awake and marks the process", () => {
  const raw = parseProbeOutput(sample({ agents: [], ttys: [], ps: [
    [1, 0, 0, 0, "?", "/sbin/init"],
    [500, 1, 1000, 1, "?", "node server.js --port 3000"],
    [501, 1, 1000, 1, "?", "python -m http.server"],
  ] }));
  const future = new Date((NOW + 3600) * 1000).toISOString();
  const pins: ActivityPin[] = [
    { id: "pin-a", pid: null, command: null, pattern: "server.js", expires_at: future },
  ];
  const s = summarizeProbe(raw, pins, { now, recentSec: 900 });
  assertEquals(s.active, true);
  assertEquals(s.reasons, ["pin"]);
  assertEquals(s.live_pins, ["pin-a"]);
  assertEquals(s.processes.find((p) => p.pid === 500)?.pinned, true);
  assertEquals(s.processes.find((p) => p.pid === 501)?.pinned, false);
});

Deno.test("expired pins, pins on gone processes and pid pins on a recycled pid do not count", () => {
  const raw = parseProbeOutput(sample({ agents: [], ttys: [], ps: [
    [1, 0, 0, 0, "?", "/sbin/init"],
    [500, 1, 1000, 1, "?", "node other.js"],
  ] }));
  const past = new Date((NOW - 1) * 1000).toISOString();
  const future = new Date((NOW + 3600) * 1000).toISOString();
  const pins: ActivityPin[] = [
    { id: "expired", pid: null, command: null, pattern: "other.js", expires_at: past },
    { id: "gone", pid: 4242, command: null, pattern: null, expires_at: future },
    { id: "recycled", pid: 500, command: "node server.js", pattern: null, expires_at: future },
  ];
  const s = summarizeProbe(raw, pins, { now, recentSec: 900 });
  assertEquals(s.active, false);
  assertEquals(s.live_pins, []);
  assertEquals(s.processes[0].pinned, false);
});

Deno.test("a pid pin matches when the command is unchanged", () => {
  const raw = parseProbeOutput(sample({ agents: [], ttys: [], ps: [[500, 1, 1000, 1, "?", "node server.js"]] }));
  const future = new Date((NOW + 60) * 1000).toISOString();
  const s = summarizeProbe(
    raw,
    [{ id: "p", pid: 500, command: "node server.js", pattern: null, expires_at: future }],
    { now, recentSec: 900 },
  );
  assertEquals(s.live_pins, ["p"]);
});

Deno.test("long commands are truncated and the list is capped", () => {
  const ps: unknown[] = [[1, 0, 0, 0, "?", "/sbin/init"]];
  for (let i = 0; i < 150; i++) ps.push([1000 + i, 1, 1000, i, "?", "x".repeat(400)]);
  const s = summarizeProbe(parseProbeOutput(sample({ ps, agents: [], ttys: [] })), [], { now, recentSec: 900 });
  assertEquals(s.processes.length, 100);
  assert(s.processes[0].command.length <= 200);
});
