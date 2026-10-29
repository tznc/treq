// An in-memory stand-in for the Fly Machines REST API
// (docs.fly.io/machines/api, base https://api.machines.dev/v1). Tests pass
// its `fetch` to FlyMachinesProvider so no request leaves the process.

export interface FakeFlyVolume {
  id: string;
  name: string;
  region: string;
  size_gb: number;
  attached_machine_id: string | null;
}

export interface FakeFlyMachine {
  id: string;
  name: string;
  state: string;
  region: string;
  config: {
    image?: string;
    guest?: { cpu_kind: string; cpus: number; memory_mb: number };
    mounts?: Array<{ volume: string; path: string }>;
    services?: Array<Record<string, unknown>>;
    metadata?: Record<string, string>;
  };
}

export interface RecordedRequest {
  method: string;
  path: string;
  headers: Headers;
  body: unknown;
}

export class FakeFly {
  readonly machines = new Map<string, FakeFlyMachine>();
  readonly volumes = new Map<string, FakeFlyVolume>();
  readonly requests: RecordedRequest[] = [];
  forcedStatus: number | null = null;
  execResult = { exit_code: 0, stdout: "", stderr: "" };
  // How many volume DELETEs a destroyed machine's volume refuses (412,
  // still attached) before it detaches. Fly detaches asynchronously.
  detachDelay = 0;
  private detaching = new Map<string, number>();
  private seq = 0;

  fetch: typeof fetch = (input, init = {}) => {
    const url = new URL(input instanceof Request ? input.url : String(input));
    const method = (init.method ?? "GET").toUpperCase();
    const body = typeof init.body === "string" ? JSON.parse(init.body) : undefined;
    this.requests.push({ method, path: url.pathname, headers: new Headers(init.headers), body });
    if (this.forcedStatus !== null) return Promise.resolve(err(this.forcedStatus, "forced"));
    return Promise.resolve(this.route(method, url.pathname, body));
  };

  private route(method: string, path: string, body: any): Response {
    // /v1/apps/{app}/{machines|volumes}[/{id}[/{action}]]
    const parts = path.split("/").filter(Boolean).map(decodeURIComponent);
    if (parts[0] !== "v1" || parts[1] !== "apps") return err(404, "not found");
    const [kind, id, action] = parts.slice(3);
    if (kind === "volumes") {
      if (!id && method === "GET") return json(200, [...this.volumes.values()]);
      if (!id && method === "POST") {
        const v: FakeFlyVolume = {
          id: `vol_${++this.seq}`,
          name: body.name,
          region: body.region,
          size_gb: body.size_gb,
          attached_machine_id: null,
        };
        this.volumes.set(v.id, v);
        return json(200, v);
      }
      const v = this.volumes.get(id);
      if (!v) return err(404, "volume not found");
      if (method === "DELETE") {
        const left = this.detaching.get(id);
        if (left !== undefined) {
          if (left > 0) {
            this.detaching.set(id, left - 1);
            return err(412, "volume is attached");
          }
          this.detaching.delete(id);
          v.attached_machine_id = null;
        }
        if (v.attached_machine_id) return err(412, "volume is attached");
        this.volumes.delete(id);
        return json(200, v);
      }
      return json(200, v);
    }
    if (kind !== "machines") return err(404, "not found");
    if (!id && method === "GET") return json(200, [...this.machines.values()]);
    if (!id && method === "POST") {
      const m: FakeFlyMachine = {
        id: `m_${++this.seq}`,
        name: body.name,
        // Fly launches a machine on create unless skip_launch is set.
        state: "started",
        region: body.region,
        config: body.config,
      };
      for (const mount of m.config.mounts ?? []) {
        const v = this.volumes.get(mount.volume);
        if (!v) return err(400, "no such volume");
        if (v.attached_machine_id) return err(400, "volume already attached");
        v.attached_machine_id = m.id;
      }
      this.machines.set(m.id, m);
      return json(200, m);
    }
    const m = this.machines.get(id);
    if (!m) return err(404, "machine not found");
    if (!action && method === "GET") return json(200, m);
    if (!action && method === "DELETE") {
      this.machines.delete(id);
      for (const v of this.volumes.values()) {
        if (v.attached_machine_id !== id) continue;
        if (this.detachDelay > 0) this.detaching.set(v.id, this.detachDelay);
        else v.attached_machine_id = null;
      }
      return json(200, { ok: true });
    }
    if (action === "start" && method === "POST") {
      m.state = "started";
      return json(200, { previous_state: "stopped" });
    }
    if (action === "stop" && method === "POST") {
      m.state = "stopped";
      return json(200, { ok: true });
    }
    if (action === "exec" && method === "POST") {
      if (m.state !== "started") return err(412, "machine not started");
      return json(200, this.execResult);
    }
    return err(404, "not found");
  }
}

function json(status: number, body: unknown): Response {
  return new Response(JSON.stringify(body), { status, headers: { "Content-Type": "application/json" } });
}

function err(status: number, message: string): Response {
  return json(status, { error: message });
}
