// An in-memory stand-in for Daytona's public REST API and its toolbox API,
// shaped after daytona.io/docs/openapi.json and
// daytona.io/docs/toolbox-openapi.json. Tests pass its `fetch` to
// DaytonaProvider so no request leaves the process.

export const FAKE_TOOLBOX_URL = "https://proxy.daytona.example.invalid/toolbox";

export interface FakeDaytonaSandbox {
  id: string;
  name: string;
  state: string;
  labels: Record<string, string>;
  cpu: number;
  memory: number;
  disk: number;
  autoStopInterval: number;
  autoArchiveInterval: number;
  autoDeleteInterval: number;
  toolboxProxyUrl: string;
}

export interface RecordedRequest {
  method: string;
  host: string;
  path: string;
  search: URLSearchParams;
  headers: Headers;
  body: unknown;
}

export class FakeDaytona {
  readonly sandboxes = new Map<string, FakeDaytonaSandbox>();
  readonly requests: RecordedRequest[] = [];
  // When set, every request answers with this status and an error envelope.
  forcedStatus: number | null = null;
  // Scripted `/process/execute` result; the default is exit 0, no output.
  executeResult: { exitCode: number; result: string } = { exitCode: 0, result: "" };
  private seq = 0;

  fetch: typeof fetch = (input, init = {}) => {
    const url = new URL(input instanceof Request ? input.url : String(input));
    const method = (init.method ?? "GET").toUpperCase();
    const headers = new Headers(init.headers);
    const body = typeof init.body === "string" ? JSON.parse(init.body) : undefined;
    this.requests.push({
      method,
      host: url.host,
      path: url.pathname,
      search: url.searchParams,
      headers,
      body,
    });
    if (this.forcedStatus !== null) {
      return Promise.resolve(errorResponse(this.forcedStatus, "forced"));
    }
    if (url.pathname.startsWith("/toolbox/")) {
      return Promise.resolve(this.toolbox(method, url.pathname, body));
    }
    return Promise.resolve(this.route(method, url.pathname.replace(/^\/api/, ""), url.searchParams, body));
  };

  private route(method: string, path: string, search: URLSearchParams, body: unknown): Response {
    const parts = path.split("/").filter(Boolean).map(decodeURIComponent);
    if (parts[0] !== "sandbox") return errorResponse(404, "not found");
    if (parts.length === 1 && method === "GET") {
      const want = JSON.parse(search.get("labels") ?? "{}") as Record<string, string>;
      const items = [...this.sandboxes.values()].filter((s) =>
        Object.entries(want).every(([k, v]) => s.labels[k] === v)
      );
      return json(200, { items, nextCursor: null });
    }
    if (parts.length === 1 && method === "POST") {
      const req = body as Partial<FakeDaytonaSandbox>;
      const id = `dt_${++this.seq}`;
      const sandbox: FakeDaytonaSandbox = {
        id,
        name: req.name ?? id,
        state: "creating",
        labels: req.labels ?? {},
        cpu: req.cpu ?? 1,
        memory: req.memory ?? 1,
        disk: req.disk ?? 3,
        autoStopInterval: req.autoStopInterval ?? 15,
        autoArchiveInterval: req.autoArchiveInterval ?? 10080,
        autoDeleteInterval: req.autoDeleteInterval ?? -1,
        toolboxProxyUrl: FAKE_TOOLBOX_URL,
      };
      this.sandboxes.set(id, sandbox);
      return json(200, sandbox);
    }
    const sandbox = this.sandboxes.get(parts[1]);
    if (!sandbox) return errorResponse(404, "sandbox not found");
    const action = parts.slice(2).join("/");
    if (action === "" && method === "GET") return json(200, sandbox);
    if (action === "" && method === "DELETE") {
      this.sandboxes.delete(sandbox.id);
      return json(200, sandbox);
    }
    if (action === "last-activity" && method === "POST") return new Response(null, { status: 201 });
    if (action === "stop" && method === "POST") {
      sandbox.state = "stopped";
      return json(200, sandbox);
    }
    if (action === "start" && method === "POST") {
      if (sandbox.state === "started") return errorResponse(400, "sandbox is already started");
      sandbox.state = "started";
      return json(200, sandbox);
    }
    const preview = action.match(/^ports\/(\d+)\/preview-url$/);
    if (preview && method === "GET") {
      return json(200, {
        sandboxId: sandbox.id,
        url: `https://${preview[1]}-${sandbox.id}.proxy.daytona.example.invalid`,
        token: "preview_tok",
      });
    }
    return errorResponse(404, "not found");
  }

  private toolbox(method: string, path: string, body: unknown): Response {
    // `{toolboxProxyUrl}/{sandboxId}/process/execute`
    const parts = path.split("/").filter(Boolean).map(decodeURIComponent);
    const sandbox = this.sandboxes.get(parts[1]);
    if (!sandbox) return errorResponse(404, "sandbox not found");
    if (sandbox.state !== "started" && sandbox.state !== "creating") {
      return errorResponse(400, "sandbox is not running");
    }
    sandbox.state = "started";
    if (parts.slice(2).join("/") === "process/execute" && method === "POST" && body) {
      return json(200, this.executeResult);
    }
    return errorResponse(404, "not found");
  }
}

function json(status: number, body: unknown): Response {
  return new Response(JSON.stringify(body), {
    status,
    headers: { "Content-Type": "application/json" },
  });
}

function errorResponse(status: number, message: string): Response {
  return json(status, { statusCode: status, message, error: message });
}
