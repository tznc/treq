// An in-memory stand-in for the parts of Cloudflare's REST API
// (api.cloudflare.com/client/v4) that cloudflare-provision.ts calls to deploy
// the sandbox bridge Worker. Tests pass its `fetch`, so nothing leaves the process.

export interface RecordedApiRequest {
  method: string;
  path: string;
  headers: Headers;
  body: unknown;
}

export interface UploadedScript {
  metadata: Record<string, unknown>;
  module: string;
}

export class FakeCloudflareApi {
  readonly requests: RecordedApiRequest[] = [];
  readonly kv = new Map<string, string>(); // id → title
  readonly applications: Array<Record<string, unknown> & { id: string }> = [];
  script: UploadedScript | null = null;
  workersDev = false;
  forcedStatus: number | null = null;
  private seq = 0;

  constructor(readonly account = "acct1", readonly token = "cf_api", readonly subdomain = "acme") {}

  fetch: typeof fetch = async (input, init = {}) => {
    const url = new URL(input instanceof Request ? input.url : String(input));
    const method = (init.method ?? "GET").toUpperCase();
    const headers = new Headers(init.headers);
    let body: unknown;
    if (typeof init.body === "string") body = JSON.parse(init.body);
    else if (init.body instanceof FormData) body = init.body;
    this.requests.push({ method, path: url.pathname, headers, body });
    if (this.forcedStatus !== null) return fail(this.forcedStatus);
    if (headers.get("Authorization") !== `Bearer ${this.token}`) return fail(401);
    const prefix = `/client/v4/accounts/${this.account}`;
    if (!url.pathname.startsWith(prefix)) return fail(403);
    return await this.route(method, url.pathname.slice(prefix.length), body);
  };

  private async route(method: string, path: string, body: unknown): Promise<Response> {
    const script = "/workers/scripts/treq-sandbox";
    if (method === "GET" && path === "/workers/subdomain") return ok({ subdomain: this.subdomain });
    if (method === "GET" && path === "/storage/kv/namespaces") {
      return ok([...this.kv].map(([id, title]) => ({ id, title })));
    }
    if (method === "POST" && path === "/storage/kv/namespaces") {
      const id = `kv${++this.seq}`;
      this.kv.set(id, (body as { title: string }).title);
      return ok({ id, title: this.kv.get(id) });
    }
    if (method === "GET" && path === `${script}/settings`) return this.script ? ok({}) : fail(404);
    if (method === "PUT" && path === script) {
      const form = body as FormData;
      const metadata = JSON.parse(await (form.get("metadata") as Blob).text());
      const module = await (form.get(metadata.main_module) as Blob).text();
      this.script = { metadata, module };
      return ok({ id: "treq-sandbox" });
    }
    if (method === "POST" && path === `${script}/subdomain`) {
      this.workersDev = (body as { enabled: boolean }).enabled;
      return ok({});
    }
    if (method === "GET" && path === "/workers/durable_objects/namespaces") {
      const classes = ((this.script?.metadata.bindings ?? []) as Array<{ type: string; class_name?: string }>)
        .filter((b) => b.type === "durable_object_namespace");
      return ok(classes.map((b) => ({ id: `ns-${b.class_name}`, class: b.class_name, script: "treq-sandbox" })));
    }
    if (method === "GET" && path === "/containers/applications") return ok(this.applications);
    if (method === "POST" && path === "/containers/applications") {
      const app = { ...(body as Record<string, unknown>), id: `app${++this.seq}` };
      this.applications.push(app);
      return ok(app);
    }
    const patch = path.match(/^\/containers\/applications\/([^/]+)$/);
    if (method === "PATCH" && patch) {
      const app = this.applications.find((a) => a.id === patch[1]);
      if (!app) return fail(404);
      Object.assign(app, body);
      return ok(app);
    }
    return fail(404);
  }
}

function ok(result: unknown): Response {
  return new Response(JSON.stringify({ success: true, errors: [], result }), {
    status: 200,
    headers: { "Content-Type": "application/json" },
  });
}

function fail(status: number): Response {
  return new Response(JSON.stringify({ success: false, errors: [{ code: status, message: "fail" }], result: null }), {
    status,
    headers: { "Content-Type": "application/json" },
  });
}
