// An in-memory stand-in for Boat's public API (docs.boat.dev/api/v1), shaped
// after the OpenAPI file at docs.boat.dev/openapi/boat-v1.yaml. Tests pass its
// `fetch` to BoatProvider so no request leaves the process.
import { defined } from "./remote_fakes.ts";


export interface FakeBoatSandbox {
  id: string;
  name: string;
  state: string;
  type: string;
  labels: Record<string, string>;
  ip: string | null;
  sshEndpoint: string | null;
  updatedAt?: string;
  stoppedAt?: string;
}

export interface RecordedRequest {
  method: string;
  path: string;
  search: URLSearchParams;
  headers: Headers;
  body: unknown;
}

export class FakeBoat {
  readonly sandboxes = new Map<string, FakeBoatSandbox>();
  readonly requests: RecordedRequest[] = [];
  readonly idempotency = new Map<string, string>();
  // When set, every request answers with this status and an error envelope.
  forcedStatus: number | null = null;
  // Scripted command result; the default is exit 0 with no output.
  commandResult: { exitCode: number; stdout: string; stderr: string } = {
    exitCode: 0,
    stdout: "",
    stderr: "",
  };
  // When set, resume answers 409 with this code.
  resumeErrorCode: string | null = null;
  private seq = 0;

  fetch: typeof fetch = (input, init = {}) => {
    const url = new URL(input instanceof Request ? input.url : String(input));
    const method = (init.method ?? "GET").toUpperCase();
    const headers = new Headers(init.headers);
    const body = typeof init.body === "string" ? JSON.parse(init.body) : undefined;
    const path = url.pathname.replace(/^\/api\/v1/, "");
    this.requests.push({ method, path, search: url.searchParams, headers, body });
    if (this.forcedStatus !== null) {
      return Promise.resolve(errorResponse(this.forcedStatus, "forced"));
    }
    return Promise.resolve(this.route(method, path, url.searchParams, headers, body));
  };

  private route(
    method: string,
    path: string,
    search: URLSearchParams,
    headers: Headers,
    body: unknown,
  ): Response {
    const parts = path.split("/").filter(Boolean).map(decodeURIComponent);
    if (parts[0] !== "sandboxes") return errorResponse(404, "not_found");
    if (parts.length === 1 && method === "GET") {
      const labels = search.getAll("label").map((l) => l.split("="));
      const sandboxes = [...this.sandboxes.values()].filter((s) =>
        labels.every(([k, v]) => s.labels[k] === v)
      );
      return json(200, { ok: true, type: "sandbox.list", sandboxes });
    }
    if (parts.length === 1 && method === "POST") {
      const key = headers.get("Idempotency-Key");
      const prior = key ? this.idempotency.get(key) : undefined;
      if (prior && this.sandboxes.has(prior)) {
        const sandbox = defined(this.sandboxes.get(prior));
        return json(201, { ok: true, type: "sandbox.created", status: "ready", ttlSeconds: null, sandbox });
      }
      const req = body as { type?: string; labels?: Record<string, string> };
      const id = `bx_${++this.seq}`;
      const sandbox: FakeBoatSandbox = {
        id,
        name: id,
        state: "provisioning",
        type: req.type ?? "default",
        labels: req.labels ?? {},
        ip: "203.0.113.10",
        sshEndpoint: null,
      };
      this.sandboxes.set(id, sandbox);
      if (key) this.idempotency.set(key, id);
      return json(201, { ok: true, type: "sandbox.created", status: "provisioning", ttlSeconds: null, sandbox });
    }
    const sandbox = this.sandboxes.get(parts[1]);
    if (!sandbox) return errorResponse(404, "not_found");
    const action = parts[2];
    if (action === undefined && method === "GET") {
      return json(200, { ok: true, type: "sandbox", sandbox });
    }
    if (action === undefined && method === "PATCH") {
      return json(200, { ok: true, type: "sandbox", sandbox });
    }
    if (action === undefined && method === "DELETE") {
      if (headers.get("X-Ascii-Confirm-Delete") !== sandbox.id) {
        return errorResponse(409, "confirm_delete_required");
      }
      this.sandboxes.delete(sandbox.id);
      return json(202, { ok: true, type: "deletion.accepted", operationId: "op_1", status: "pending" });
    }
    if (action === "stop" && method === "POST") {
      sandbox.state = "archived";
      return json(202, { ok: true, type: "sandbox.stopping", sandbox });
    }
    if (action === "resume" && method === "POST") {
      if (this.resumeErrorCode) return errorResponse(409, this.resumeErrorCode);
      if (sandbox.state !== "archived") return errorResponse(409, "sandbox_running");
      sandbox.state = "ready";
      return json(202, { ok: true, type: "sandbox.resuming", sandbox });
    }
    if (action === "commands" && method === "POST") {
      if (sandbox.state === "archived") return errorResponse(409, "sandbox_not_ready");
      sandbox.state = "running";
      return json(200, {
        ok: true,
        type: "command.finished",
        success: this.commandResult.exitCode === 0,
        timedOut: false,
        ...this.commandResult,
      });
    }
    if (action === "host" && method === "POST") {
      if (sandbox.state === "archived") return errorResponse(409, "sandbox_not_ready");
      // Boat's hosted URL carries the port token as `?_token=`, which a
      // browser trades for a `_port_auth` cookie through a redirect.
      const req = body as { port: number };
      return json(200, {
        ok: true,
        type: "sandbox.host",
        url: `https://${sandbox.id}-${req.port}.boat.example.invalid/?_token=host_tok`,
      });
    }
    return errorResponse(404, "not_found");
  }
}

function json(status: number, body: unknown): Response {
  return new Response(JSON.stringify(body), {
    status,
    headers: { "Content-Type": "application/json" },
  });
}

function errorResponse(status: number, code: string): Response {
  return json(status, {
    ok: false,
    type: "sandbox.error",
    status,
    code,
    message: code,
    requestId: "req_test",
    error: { code, message: code, status },
  });
}
