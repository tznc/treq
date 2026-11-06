// treq sandbox bridge: the REST API that treq's Cloudflare adapter
// (supabase/functions/_shared/remote/cloudflare-adapter.ts) calls. Deploy it
// to your own Cloudflare account, set BRIDGE_TOKEN with
// `wrangler secret put BRIDGE_TOKEN`, then save
// `https://treq-sandbox.<you>.workers.dev#<token>` as your Cloudflare key in
// treq's Settings → Provider keys.
//
// UNVERIFIED against a live account: getState() status names and wsConnect
// carrying binary frames.

import { getSandbox, Sandbox } from "@cloudflare/sandbox";

export class SandboxS extends Sandbox {}
export class SandboxM extends Sandbox {}

interface Env {
  SANDBOX_S: DurableObjectNamespace<SandboxS>;
  SANDBOX_M: DurableObjectNamespace<SandboxM>;
  TYPES: KVNamespace;
  BRIDGE_TOKEN: string;
}

const json = (status: number, body: unknown) =>
  new Response(JSON.stringify(body), { status, headers: { "Content-Type": "application/json" } });

function sameToken(a: string, b: string): boolean {
  if (a.length !== b.length) return false;
  let diff = 0;
  for (let i = 0; i < a.length; i++) diff |= a.charCodeAt(i) ^ b.charCodeAt(i);
  return diff === 0;
}

function bridgeState(status: string | undefined): string {
  if (status === "running" || status === "healthy") return "running";
  if (status === "starting") return "starting";
  return "sleeping";
}

export default {
  async fetch(request: Request, env: Env): Promise<Response> {
    const auth = request.headers.get("Authorization") ?? "";
    if (!env.BRIDGE_TOKEN || !sameToken(auth, `Bearer ${env.BRIDGE_TOKEN}`)) return json(401, { error: "unauthorized" });
    const url = new URL(request.url);
    if (url.pathname === "/v1/health") return json(200, { ok: true });
    const m = url.pathname.match(/^\/v1\/sandboxes\/([a-z0-9._-]{1,80})(?:\/([a-z]+)(?:\/(\d+))?)?$/);
    if (!m) return json(404, { error: "no route" });
    const [, id, action = "", port] = m;
    const method = request.method;

    // The tier is fixed at create; it picks the container class.
    let type = await env.TYPES.get(id);
    if (method === "PUT" && !action) {
      const body = await request.json() as { instanceType: string; sleepAfterSec: number };
      if (!type) {
        if (body.instanceType !== "s" && body.instanceType !== "m") return json(400, { error: "instanceType" });
        type = body.instanceType;
        await env.TYPES.put(id, type);
      }
      const sandbox = getSandbox(type === "m" ? env.SANDBOX_M : env.SANDBOX_S, id, { sleepAfter: body.sleepAfterSec });
      await sandbox.exec("true");
      return json(201, { id, state: "running" });
    }
    if (!type) return json(404, { error: "not found" });
    const sandbox = getSandbox(type === "m" ? env.SANDBOX_M : env.SANDBOX_S, id);

    if (method === "GET" && !action) return json(200, { id, state: bridgeState((await sandbox.getState()).status) });
    if (method === "DELETE" && !action) {
      await sandbox.destroy();
      await env.TYPES.delete(id);
      return new Response(null, { status: 204 });
    }
    if (method === "POST" && action === "start") {
      await sandbox.exec("true");
      return json(200, { id, state: "running" });
    }
    if (method === "POST" && action === "stop") {
      await sandbox.stop();
      return json(200, { id, state: "sleeping" });
    }
    if (method === "POST" && action === "keepalive") {
      const { sleepAfterSec } = await request.json() as { sleepAfterSec: number };
      await sandbox.setSleepAfter(sleepAfterSec);
      return json(200, { id, state: "running" });
    }
    if (method === "POST" && action === "exec") {
      const { argv, env: vars, timeoutMs } = await request.json() as {
        argv: string[];
        env: Record<string, string>;
        timeoutMs: number;
      };
      const quoted = argv.map((a) => `'${a.replaceAll("'", `'\\''`)}'`).join(" ");
      const r = await sandbox.exec(quoted, { env: vars, timeout: timeoutMs });
      return json(200, { exitCode: r.exitCode, stdout: r.stdout, stderr: r.stderr });
    }
    if (method === "GET" && action === "ports" && port && request.headers.get("Upgrade") === "websocket") {
      return await sandbox.wsConnect(request, Number(port));
    }
    return json(404, { error: "no route" });
  },
};
