// An in-memory stand-in for the treq sandbox bridge Worker
// (cloudflare/sandbox-bridge), which wraps Cloudflare's Sandbox SDK behind a
// small REST API. Tests pass its `fetch` to CloudflareProvider so no request
// leaves the process.

export interface FakeCloudflareSandbox {
  id: string;
  instanceType: string;
  state: "running" | "sleeping";
  labels: Record<string, string>;
  sleepAfterSec: number;
}

export interface RecordedRequest {
  method: string;
  host: string;
  path: string;
  headers: Headers;
  body: unknown;
}

export class FakeCloudflare {
  readonly sandboxes = new Map<string, FakeCloudflareSandbox>();
  readonly requests: RecordedRequest[] = [];
  forcedStatus: number | null = null;
  commandResult = { exitCode: 0, stdout: "", stderr: "" };

  constructor(readonly token = "cf_test") {}

  fetch: typeof fetch = async (input, init = {}) => {
    const url = new URL(input instanceof Request ? input.url : String(input));
    const method = (init.method ?? "GET").toUpperCase();
    const headers = new Headers(init.headers);
    const body = typeof init.body === "string" ? JSON.parse(init.body) : undefined;
    this.requests.push({ method, host: url.host, path: url.pathname, headers, body });
    if (this.forcedStatus !== null) return json(this.forcedStatus, { error: "forced" });
    if (headers.get("Authorization") !== `Bearer ${this.token}`) return json(401, { error: "unauthorized" });
    return await Promise.resolve(this.route(method, url.pathname, body));
  };

  private route(method: string, path: string, body: unknown): Response {
    if (path === "/v1/health" && method === "GET") return json(200, { ok: true });
    const m = path.match(/^\/v1\/sandboxes\/([^/]+)(\/[a-z]+)?$/);
    if (!m) return json(404, { error: "no route" });
    const id = decodeURIComponent(m[1]);
    const action = m[2] ?? "";
    const sandbox = this.sandboxes.get(id);
    if (method === "PUT" && action === "") {
      if (sandbox) return json(200, view(sandbox));
      const req = body as { instanceType: string; labels?: Record<string, string>; sleepAfterSec: number };
      const created: FakeCloudflareSandbox = {
        id,
        instanceType: req.instanceType,
        state: "running",
        labels: req.labels ?? {},
        sleepAfterSec: req.sleepAfterSec,
      };
      this.sandboxes.set(id, created);
      return json(201, view(created));
    }
    if (!sandbox) return json(404, { error: "not found" });
    if (method === "GET" && action === "") return json(200, view(sandbox));
    if (method === "DELETE" && action === "") {
      this.sandboxes.delete(id);
      return new Response(null, { status: 204 });
    }
    if (method === "POST" && action === "/start") {
      sandbox.state = "running";
      return json(200, view(sandbox));
    }
    if (method === "POST" && action === "/stop") {
      sandbox.state = "sleeping";
      return json(200, view(sandbox));
    }
    if (method === "POST" && action === "/keepalive") {
      sandbox.sleepAfterSec = (body as { sleepAfterSec: number }).sleepAfterSec;
      return json(200, view(sandbox));
    }
    if (method === "POST" && action === "/exec") {
      sandbox.state = "running";
      return json(200, this.commandResult);
    }
    return json(404, { error: "no route" });
  }
}

function view(s: FakeCloudflareSandbox) {
  return { id: s.id, state: s.state, instanceType: s.instanceType };
}

function json(status: number, body: unknown): Response {
  return new Response(JSON.stringify(body), { status, headers: { "Content-Type": "application/json" } });
}
