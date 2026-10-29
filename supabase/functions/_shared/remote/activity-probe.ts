// The in-sandbox activity probe and its parser, used by the idle controller
// (idle-controller.ts) to decide whether a cloud workspace is in use.
//
// The probe is a bash script the control plane runs through
// `execOnMachine`. It only collects facts and prints one JSON object; every
// decision is made here, server-side, where it can be tested:
//
// * `ps`: every process as [pid, ppid, uid, cpu%, tty, args]. cpu% is ps's
//   lifetime average, not a live sample.
// * `services`: pids from the pidfile supervisor's /var/run/treq/*.pid
//   (Boat, Daytona `ensureService`).
// * `agents`: [pid, log mtime] from each `.treq/agents/<workspace>.json`
//   record (core::agent_supervisor) under $HOME.
// * `ttys`: [path, atime, mtime] of each /dev/pts device, which is how `w`
//   computes terminal idle time.
// * `listen`: [pid, port] for listening TCP sockets, from `ss`.
//
// What keeps a workspace awake (owner decision, option B): an open sshd
// session, an agent turn (a live agent whose log was written recently), a
// terminal session of the pty daemon (core::pty_daemon) whose tty saw input
// or output recently, or a live pin. Unpinned background processes never
// do, however busy.

export const PROBE_VERSION = 1;

// Kept POSIX-ish and dependency-free: ps, find, stat, awk and sed are on
// every image we run; ss is optional.
// UNVERIFIED: that the provider exec user can read other users' entries in
// `ss -p` (it reports pids only for the caller's own sockets unless root).
export const PROBE_SCRIPT = String.raw`
set -u
esc() { sed -e 's/\\/\\\\/g' -e 's/"/\\"/g'; }
now=$(date +%s)
printf '{"v":1,"now":%s,"self":%s,"ps":[' "$now" "$$"
ps -eo pid=,ppid=,uid=,pcpu=,tty=,args= 2>/dev/null | awk '
  { pid=$1; ppid=$2; uid=$3; cpu=$4; tty=$5; $1=$2=$3=$4=$5=""; sub(/^ +/, "");
    a=substr($0, 1, 400); gsub(/\\/, "\\\\", a); gsub(/"/, "\\\"", a); gsub(/\t/, " ", a);
    printf "%s[%d,%d,%d,%s,\"%s\",\"%s\"]", (n++ ? "," : ""), pid, ppid, uid, cpu + 0, tty, a }'
printf '],"services":['
sep=""
for f in /var/run/treq/*.pid; do
  [ -f "$f" ] || continue
  p=$(tr -dc '0-9' < "$f")
  [ -n "$p" ] && { printf '%s%s' "$sep" "$p"; sep=","; }
done
printf '],"agents":['
sep=""
find "$HOME" -maxdepth 6 -path '*/.treq/agents/*.json' -type f 2>/dev/null | while read -r rec; do
  body=$(tr -d '\n' < "$rec")
  p=$(printf '%s' "$body" | sed -n 's/.*"pid"[[:space:]]*:[[:space:]]*\([0-9][0-9]*\).*/\1/p')
  log=$(printf '%s' "$body" | sed -n 's/.*"log_path"[[:space:]]*:[[:space:]]*"\([^"]*\)".*/\1/p')
  [ -n "$p" ] || continue
  m=0
  [ -n "$log" ] && [ -f "$log" ] && m=$(stat -c %Y "$log" 2>/dev/null || echo 0)
  printf '%s[%s,%s]' "$sep" "$p" "$m"; sep=","
done
printf '],"ttys":['
sep=""
for t in /dev/pts/[0-9]*; do
  [ -e "$t" ] || continue
  printf '%s["%s",%s]' "$sep" "$t" "$(stat -c '%X,%Y' "$t" 2>/dev/null || echo 0,0)"; sep=","
done
printf '],"listen":['
ss -ltnpH 2>/dev/null | awk '
  { port=$4; sub(/.*:/, "", port); s=$0;
    while (match(s, /pid=[0-9]+/)) { printf "%s[%s,%s]", (n++ ? "," : ""), substr(s, RSTART + 4, RLENGTH - 4), port + 0; s=substr(s, RSTART + RLENGTH) } }'
printf ']}\n'
`;

export function probeCommand(): string[] {
  return ["bash", "-c", PROBE_SCRIPT];
}

export class ProbeParseError extends Error {}

/** [pid, ppid, uid, cpu%, tty, args] */
export type ProbeProcess = [number, number, number, number, string, string];

export interface RawProbe {
  now: number;
  self: number;
  ps: ProbeProcess[];
  services: number[];
  /** [pid, log mtime] */
  agents: Array<[number, number]>;
  /** [path, atime, mtime] */
  ttys: Array<[string, number, number]>;
  /** [pid, port] */
  listen: Array<[number, number]>;
}

function arrayOr(value: unknown, what: string): unknown[] {
  if (value === undefined) return [];
  if (!Array.isArray(value)) throw new ProbeParseError(`${what} is not a list`);
  return value;
}

/** Parses the probe's stdout; anything printed before the JSON line is ignored. */
export function parseProbeOutput(stdout: string): RawProbe {
  const start = stdout.indexOf('{"v":');
  if (start < 0) throw new ProbeParseError("no probe output");
  let body: Record<string, unknown>;
  try {
    body = JSON.parse(stdout.slice(start).trim());
  } catch {
    throw new ProbeParseError("probe output is not JSON");
  }
  if (body.v !== PROBE_VERSION) throw new ProbeParseError(`unsupported probe version ${String(body.v)}`);
  if (typeof body.now !== "number" || typeof body.self !== "number") {
    throw new ProbeParseError("probe output lacks now/self");
  }
  if (!Array.isArray(body.ps)) throw new ProbeParseError("ps is not a list");
  const ps = (body.ps as unknown[]).filter((r): r is ProbeProcess =>
    Array.isArray(r) && r.length === 6 && typeof r[0] === "number" && typeof r[5] === "string"
  );
  return {
    now: body.now,
    self: body.self,
    ps,
    services: arrayOr(body.services, "services").filter((p): p is number => typeof p === "number"),
    agents: arrayOr(body.agents, "agents") as Array<[number, number]>,
    ttys: arrayOr(body.ttys, "ttys") as Array<[string, number, number]>,
    listen: arrayOr(body.listen, "listen") as Array<[number, number]>,
  };
}

/** A pin row (remote_process_pins): by pid (with the command seen then) or by substring. */
export interface ActivityPin {
  id: string;
  pid: number | null;
  command: string | null;
  pattern: string | null;
  expires_at: string;
}

export type ProcessOrigin = "service" | "agent" | "terminal" | "orphan";

export interface ProbeProcessInfo {
  pid: number;
  command: string;
  origin: ProcessOrigin;
  cpu: number;
  ports: number[];
  pinned: boolean;
}

export type ActivityReason = "ssh" | "agent" | "terminal" | "pin";

export interface ProbeSummary {
  ssh_sessions: number;
  agent_turns: number;
  pty_sessions: number;
  active: boolean;
  reasons: ActivityReason[];
  /**
   * Seconds since the last activity the probe can see, on the sandbox's
   * clock: 0 with an ssh session or live pin, else the age of the freshest
   * agent log write or terminal touch; null when there is no evidence.
   */
  idle_sec: number | null;
  live_pins: string[];
  processes: ProbeProcessInfo[];
}

const MAX_PROCESSES = 100;
const MAX_COMMAND = 200;
const SSH_SESSION = /^sshd(-session)?: \S+@/;
const SSH_PRIV = /^sshd(-session)?: /;
const PTY_DAEMON = /pty-remote daemon/;

export function summarizeProbe(
  raw: RawProbe,
  pins: readonly ActivityPin[],
  opts: { now: Date; recentSec: number },
): ProbeSummary {
  const byPid = new Map<number, ProbeProcess>(raw.ps.map((p) => [p[0], p]));
  const services = new Set(raw.services);
  const ptyDaemons = new Set(raw.ps.filter((p) => PTY_DAEMON.test(p[5])).map((p) => p[0]));
  const liveAgents = raw.agents.filter(([pid]) => byPid.has(pid));
  const agentPids = new Set(liveAgents.map(([pid]) => pid));

  // The probe, the exec wrapper that started it, and everything under it.
  const hidden = new Set<number>();
  for (let pid = raw.self; pid > 1 && byPid.has(pid) && !hidden.has(pid); pid = byPid.get(pid)![1]) {
    hidden.add(pid);
  }
  const underSelf = (pid: number): boolean => {
    for (let p = pid, n = 0; p > 1 && n < 64; p = byPid.get(p)?.[1] ?? 0, n++) {
      if (p === raw.self) return true;
    }
    return false;
  };

  const originOf = (pid: number): ProcessOrigin => {
    for (let p = pid, n = 0; p > 1 && n < 64; p = byPid.get(p)?.[1] ?? 0, n++) {
      if (services.has(p)) return "service";
      if (agentPids.has(p)) return "agent";
      if (ptyDaemons.has(p) && p !== pid) return "terminal";
    }
    return "orphan";
  };

  const ports = new Map<number, number[]>();
  for (const [pid, port] of raw.listen) {
    const list = ports.get(pid) ?? [];
    if (!list.includes(port)) list.push(port);
    ports.set(pid, list.sort((a, b) => a - b));
  }

  const nowMs = opts.now.getTime();
  const unexpired = pins.filter((pin) => Date.parse(pin.expires_at) > nowMs);
  const pinMatches = (pin: ActivityPin, pid: number, args: string): boolean => {
    if (pin.pid !== null) return pin.pid === pid && (pin.command === null || pin.command === args);
    return pin.pattern !== null && pin.pattern.length > 0 && args.includes(pin.pattern);
  };

  const livePins = new Set<string>();
  const processes: ProbeProcessInfo[] = [];
  for (const [pid, , uid, cpu, , args] of raw.ps) {
    if (pid <= 1 || hidden.has(pid) || underSelf(pid)) continue;
    const pinnedBy = unexpired.filter((pin) => pinMatches(pin, pid, args));
    pinnedBy.forEach((pin) => livePins.add(pin.id));
    if (SSH_PRIV.test(args) || PTY_DAEMON.test(args)) continue;
    const origin = originOf(pid);
    if (origin === "orphan" && uid < 1000) continue;
    processes.push({
      pid,
      command: args.slice(0, MAX_COMMAND),
      origin,
      cpu: Number(cpu) || 0,
      ports: ports.get(pid) ?? [],
      pinned: pinnedBy.length > 0,
    });
  }
  processes.sort((a, b) => b.cpu - a.cpu || a.pid - b.pid);

  const sshSessions = raw.ps.filter((p) => SSH_SESSION.test(p[5])).length;
  const agentTurns = liveAgents.filter(([, mtime]) => raw.now - mtime < opts.recentSec).length;
  const ttyTouched = new Map(raw.ttys.map(([path, atime, mtime]) => [path, Math.max(atime, mtime)]));
  const ptyAges = raw.ps
    .filter((p) => ptyDaemons.has(p[1]) && p[4].startsWith("pts/"))
    .map((p) => raw.now - (ttyTouched.get(`/dev/${p[4]}`) ?? 0));
  const ptySessions = ptyAges.filter((age) => age < opts.recentSec).length;
  const ages = [
    ...liveAgents.filter(([, mtime]) => mtime > 0).map(([, mtime]) => raw.now - mtime),
    ...ptyAges.filter((age) => age < raw.now),
  ].map((age) => Math.max(0, age));

  const reasons: ActivityReason[] = [];
  if (sshSessions > 0) reasons.push("ssh");
  if (agentTurns > 0) reasons.push("agent");
  if (ptySessions > 0) reasons.push("terminal");
  if (livePins.size > 0) reasons.push("pin");

  return {
    ssh_sessions: sshSessions,
    agent_turns: agentTurns,
    pty_sessions: ptySessions,
    active: reasons.length > 0,
    reasons,
    idle_sec: sshSessions > 0 || livePins.size > 0 ? 0 : ages.length > 0 ? Math.min(...ages) : null,
    live_pins: [...livePins],
    processes: processes.slice(0, MAX_PROCESSES),
  };
}
