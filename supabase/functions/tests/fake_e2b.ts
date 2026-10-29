// An in-memory stand-in for E2B's public REST API (spec/openapi.yml in
// github.com/e2b-dev/E2B) and the envd process service (Connect RPC
// `process.Process/Start` on port 49983). Tests pass its `fetch` to
// E2BProvider so no request leaves the process.

export interface FakeE2BSandbox {
  sandboxID: string;
  templateID: string;
  state: "running" | "paused";
  metadata: Record<string, string>;
  envdAccessToken: string;
  trafficAccessToken: string;
  domain: string;
  // E2B's TTL in seconds, as last set by create, connect or set-timeout.
  timeout: number;
}

export interface RecordedRequest {
  method: string;
  host: string;
  path: string;
  search: URLSearchParams;
  headers: Headers;
  body: unknown;
}

export class FakeE2B {
  readonly sandboxes = new Map<string, FakeE2BSandbox>();
  readonly requests: RecordedRequest[] = [];
  readonly idempotency = new Map<string, string>();
  forcedStatus: number | null = null;
  commandResult = { exitCode: 0, stdout: "", stderr: "" };
  private seq = 0;

  fetch: typeof fetch = async (input, init = {}) => {
    const url = new URL(input instanceof Request ? input.url : String(input));
    const method = (init.method ?? "GET").toUpperCase();
    const headers = new Headers(init.headers);
    let body: unknown;
    if (typeof init.body === "string") body = JSON.parse(init.body);
    else if (init.body instanceof Uint8Array) body = decodeEnvelopes(init.body)[0]?.json;
    this.requests.push({ method, host: url.host, path: url.pathname, search: url.searchParams, headers, body });
    if (this.forcedStatus !== null) return errorResponse(this.forcedStatus, "forced");
    const envd = url.host.match(/^49983-([^.]+)\./);
    if (envd) return this.envd(envd[1], url.pathname, headers, body);
    if (headers.get("X-API-Key") !== "e2b_test") return errorResponse(401, "unauthorized");
    return await Promise.resolve(this.route(method, url.pathname, url.searchParams, headers, body));
  };

  private route(
    method: string,
    path: string,
    search: URLSearchParams,
    headers: Headers,
    body: unknown,
  ): Response {
    if (path === "/v2/sandboxes" && method === "GET") {
      const filter = new URLSearchParams(search.get("metadata") ?? "");
      const states = (search.get("state") ?? "running,paused").split(",");
      const list = [...this.sandboxes.values()].filter((s) =>
        states.includes(s.state) && [...filter].every(([k, v]) => s.metadata[k] === v)
      );
      return json(200, list.map(({ envdAccessToken: _e, trafficAccessToken: _t, ...s }) => s));
    }
    if (path === "/v2/sandboxes" && method === "POST") {
      const key = headers.get("Idempotency-Key");
      const prior = key ? this.idempotency.get(key) : undefined;
      if (prior && this.sandboxes.has(prior)) return json(201, this.sandboxes.get(prior));
      const req = body as { templateID: string; metadata?: Record<string, string> };
      const id = `sbx${++this.seq}`;
      const sandbox: FakeE2BSandbox = {
        sandboxID: id,
        templateID: req.templateID,
        state: "running",
        metadata: req.metadata ?? {},
        envdAccessToken: `envd-${id}`,
        trafficAccessToken: `traffic-${id}`,
        domain: "e2b.app",
        timeout: (body as { timeout?: number }).timeout ?? 300,
      };
      this.sandboxes.set(id, sandbox);
      if (key) this.idempotency.set(key, id);
      return json(201, sandbox);
    }
    const m = path.match(/^\/(v2\/)?sandboxes\/([^/]+)(\/[a-z]+)?$/);
    const sandbox = m ? this.sandboxes.get(decodeURIComponent(m[2])) : undefined;
    if (!m || !sandbox) return errorResponse(404, "not_found");
    const action = m[3];
    if (!action && method === "GET") {
      const { trafficAccessToken: _t, ...detail } = sandbox;
      return json(200, detail);
    }
    if (!action && method === "DELETE") {
      this.sandboxes.delete(sandbox.sandboxID);
      return new Response(null, { status: 204 });
    }
    if (action === "/pause" && method === "POST") {
      if (sandbox.state === "paused") return errorResponse(409, "already paused");
      sandbox.state = "paused";
      return new Response(null, { status: 204 });
    }
    if (action === "/timeout" && method === "POST") {
      if (sandbox.state !== "running") return errorResponse(404, "sandbox not running");
      sandbox.timeout = (body as { timeout: number }).timeout;
      return new Response(null, { status: 204 });
    }
    if (action === "/connect" && method === "POST") {
      const was = sandbox.state;
      sandbox.state = "running";
      // Connect only extends the TTL.
      const t = (body as { timeout?: number } | undefined)?.timeout ?? 300;
      sandbox.timeout = Math.max(sandbox.timeout, t);
      return json(was === "running" ? 200 : 201, sandbox);
    }
    return errorResponse(404, "not_found");
  }

  private envd(id: string, path: string, headers: Headers, _body: unknown): Response {
    const sandbox = this.sandboxes.get(id);
    if (!sandbox || sandbox.state !== "running") return new Response("not found", { status: 502 });
    if (headers.get("X-Access-Token") !== sandbox.envdAccessToken) {
      return new Response("unauthorized", { status: 401 });
    }
    if (path !== "/process.Process/Start") return new Response("", { status: 404 });
    const b64 = (s: string) => btoa(s);
    const events: unknown[] = [{ event: { start: { pid: 7 } } }];
    if (this.commandResult.stdout) events.push({ event: { data: { stdout: b64(this.commandResult.stdout) } } });
    if (this.commandResult.stderr) events.push({ event: { data: { stderr: b64(this.commandResult.stderr) } } });
    events.push({
      event: {
        end: {
          ...(this.commandResult.exitCode ? { exitCode: this.commandResult.exitCode } : {}),
          exited: true,
          status: `exit status ${this.commandResult.exitCode}`,
        },
      },
    });
    const frames = events.map((e) => envelope(0, e));
    frames.push(envelope(2, {}));
    return new Response(concat(frames), {
      status: 200,
      headers: { "Content-Type": "application/connect+json" },
    });
  }
}

export function envelope(flags: number, message: unknown): Uint8Array {
  const payload = new TextEncoder().encode(JSON.stringify(message));
  const out = new Uint8Array(5 + payload.length);
  out[0] = flags;
  new DataView(out.buffer).setUint32(1, payload.length);
  out.set(payload, 5);
  return out;
}

export function decodeEnvelopes(bytes: Uint8Array): Array<{ flags: number; json: unknown }> {
  const out: Array<{ flags: number; json: unknown }> = [];
  let off = 0;
  while (off + 5 <= bytes.length) {
    const len = new DataView(bytes.buffer, bytes.byteOffset + off + 1, 4).getUint32(0);
    const text = new TextDecoder().decode(bytes.subarray(off + 5, off + 5 + len));
    out.push({ flags: bytes[off], json: JSON.parse(text) });
    off += 5 + len;
  }
  return out;
}

function concat(parts: Uint8Array[]): Uint8Array<ArrayBuffer> {
  const out = new Uint8Array(parts.reduce((n, p) => n + p.length, 0));
  let off = 0;
  for (const p of parts) {
    out.set(p, off);
    off += p.length;
  }
  return out;
}

function json(status: number, body: unknown): Response {
  return new Response(JSON.stringify(body), { status, headers: { "Content-Type": "application/json" } });
}

function errorResponse(status: number, message: string): Response {
  return json(status, { code: status, message });
}
