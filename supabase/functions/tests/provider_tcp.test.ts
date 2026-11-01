// The one shared openTcp: endpoint from the adapter, the gateway token, the
// gateway proxy handshake, and one wake-and-retry.
// No network.

import { defined } from "./remote_fakes.ts";
import { assert, assertEquals, assertRejects } from "https://deno.land/std@0.224.0/assert/mod.ts";
import { GATEWAY_TOKEN_HEADER, verifyGatewayToken } from "../_shared/remote/gateway-token.ts";
import { openTcp } from "../_shared/remote/provider-tcp.ts";
import { BOAT_CAPABILITIES } from "../_shared/remote/boat-adapter.ts";
import { ProviderError, type SandboxProvider } from "../_shared/remote/provider.ts";
import type { RelaySocket } from "../_shared/remote/gateway-proxy.ts";

// Answers the proxy handshake on the next tick: connected, or a close.
class HandshakeSocket implements RelaySocket {
  readyState = 0;
  bufferedAmount = 0;
  binaryType: BinaryType = "blob";
  sent: string[] = [];
  private listeners = new Map<string, Array<(e: never) => void>>();
  constructor(ok: boolean) {
    setTimeout(() => {
      if (!ok) return this.emit("close", { code: 1006 });
      this.readyState = 1;
      this.emit("open", {});
    }, 0);
  }
  addEventListener(type: string, l: (e: never) => void): void {
    this.listeners.set(type, [...(this.listeners.get(type) ?? []), l]);
  }
  private emit(type: string, e: unknown) {
    for (const l of this.listeners.get(type) ?? []) l(e as never);
  }
  send(data: ArrayBufferLike | ArrayBufferView | string): void {
    this.sent.push(String(data));
    setTimeout(() => this.emit("message", { data: JSON.stringify({ status: "connected" }) }), 0);
  }
  close(): void {
    this.readyState = 3;
  }
}

function gatewayProvider(endpointCalls: string[]): SandboxProvider {
  return {
    kind: "boat",
    capabilities: () => ({ ...BOAT_CAPABILITIES }),
    gatewayEndpoint: (id: string) => {
      endpointCalls.push(id);
      return Promise.resolve({ url: "wss://gw.example.invalid/?_token=t", headers: { "x-vendor": "v" } });
    },
  } as unknown as SandboxProvider;
}

const CRED = { placementId: "11111111-2222-3333-4444-555555555555", secret: "s3cret" };

Deno.test("gateway provider: adds a valid x-treq-gateway-token for the placement", async () => {
  const calls: string[] = [];
  let headers: Record<string, string> = {};
  let socket = null as HandshakeSocket | null;
  await openTcp(gatewayProvider(calls), "bx_1", "127.0.0.1", 2222, (_url, h) => {
    headers = h;
    socket = new HandshakeSocket(true);
    return socket;
  }, { credential: CRED });
  assertEquals(defined(socket).sent[0], JSON.stringify({ host: "127.0.0.1", port: 2222 }));
  assertEquals(headers["x-vendor"], "v");
  const token = headers[GATEWAY_TOKEN_HEADER];
  assert(token);
  assertEquals(
    await verifyGatewayToken(CRED.secret, CRED.placementId, token, Math.floor(Date.now() / 1000)),
    "ok",
  );
});

Deno.test("gateway provider: refuses without a gateway credential", async () => {
  const err = await assertRejects(
    () => openTcp(gatewayProvider([]), "bx_1", "127.0.0.1", 2222, () => new HandshakeSocket(true)),
    ProviderError,
  );
  assertEquals(err.kind, "unavailable");
});

Deno.test("wakes and retries once, asking for a fresh endpoint", async () => {
  const calls: string[] = [];
  const woken: string[] = [];
  let n = 0;
  await openTcp(gatewayProvider(calls), "bx_1", "127.0.0.1", 2222, () => new HandshakeSocket(n++ > 0), {
    credential: CRED,
    wake: (id) => {
      woken.push(id);
      return Promise.resolve();
    },
  });
  assertEquals(woken, ["bx_1"]);
  assertEquals(calls, ["bx_1", "bx_1"]);
});
