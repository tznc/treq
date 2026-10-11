// Local unit tests for the managed SSH relay (`_shared/remote/ssh-relay.ts`)
// and the sshd host-key parsing it depends on. No network: sockets and the
// database are fakes.
//
// Run with: `deno test --allow-env supabase/functions/tests/remote_ssh_relay.test.ts`

import {
  assert,
  assertEquals,
  assertRejects,
} from "https://deno.land/std@0.224.0/assert/mod.ts";
import {
  authorizeRelayRequest,
  openUpstream,
  openUpstreamWithWake,
  pipeSockets,
  RelayError,
  type RelaySocket,
  type RelayStore,
  type RelayTarget,
  relayUrlForEndpoint,
  UpstreamError,
} from "../_shared/remote/ssh-relay.ts";
import { parseHostKeyLines } from "../_shared/remote/managed-sshd.ts";
import { spritesProxyUrl } from "../_shared/remote/sprites-adapter.ts";

const OWNER = "user-1";

function store(
  overrides: Partial<{
    endpoint: Record<string, unknown> | null;
    instance: Record<string, unknown> | null;
    key: Record<string, unknown> | null;
  }> = {},
): RelayStore & { calls: string[] } {
  const calls: string[] = [];
  const endpoint =
    "endpoint" in overrides
      ? overrides.endpoint
      : {
          id: "ep-1",
          instance_id: "inst-1",
          source: "managed",
          hostname: "localhost",
          port: 2222,
        };
  const instance =
    "instance" in overrides
      ? overrides.instance
      : {
          id: "inst-1",
          status: "ready",
          provider_resource_id: "dev-treq-user-1",
        };
  const key =
    "key" in overrides ? overrides.key : { id: "key-1", revoked_at: null };
  return {
    calls,
    // Each fake only answers for OWNER, like the owner-filtered queries.
    getEndpoint: (owner, id) => {
      calls.push(`endpoint:${owner}:${id}`);
      return Promise.resolve(
        owner === OWNER && id === "ep-1" ? (endpoint as never) : null,
      );
    },
    getInstance: (owner, id) => {
      calls.push(`instance:${owner}:${id}`);
      return Promise.resolve(owner === OWNER ? (instance as never) : null);
    },
    getClientKey: (owner, id) => {
      calls.push(`key:${owner}:${id}`);
      return Promise.resolve(
        owner === OWNER && id === "key-1" ? (key as never) : null,
      );
    },
    // Pro refusals are covered in test/billing/relay-pro.test.ts.
    hasPro: () => Promise.resolve(true),
  };
}

function relayRequest(
  query = "endpoint_id=ep-1&key_id=key-1",
  auth: string | null = "Bearer good-jwt",
): Request {
  const headers = new Headers({ upgrade: "websocket" });
  if (auth) headers.set("authorization", auth);
  return new Request(
    `https://example.test/functions/v1/remote-ssh-relay?${query}`,
    { headers },
  );
}

const getUserId = (jwt: string) =>
  Promise.resolve(
    jwt === "good-jwt" ? OWNER : jwt === "other-jwt" ? "user-2" : null,
  );

async function expectRelayError(promise: Promise<unknown>, status: number) {
  const err = await assertRejects(
    () => promise as Promise<unknown>,
    RelayError,
  );
  assertEquals((err as RelayError).status, status);
}

Deno.test("relay authorizes the owner of a ready managed endpoint", async () => {
  const target = await authorizeRelayRequest(relayRequest(), {
    getUserId,
    store: store(),
  });
  assertEquals(target, {
    ownerUserId: OWNER,
    endpointId: "ep-1",
    instanceId: "inst-1",
    spriteName: "dev-treq-user-1",
    host: "localhost",
    port: 2222,
  });
});

Deno.test("relay rejects a missing or invalid JWT before any lookup", async () => {
  const s = store();
  await expectRelayError(
    authorizeRelayRequest(relayRequest(undefined, null), {
      getUserId,
      store: s,
    }),
    401,
  );
  await expectRelayError(
    authorizeRelayRequest(relayRequest(undefined, "Bearer bad"), {
      getUserId,
      store: s,
    }),
    401,
  );
  assertEquals(s.calls, []);
});

Deno.test("relay requires endpoint_id and key_id", async () => {
  await expectRelayError(
    authorizeRelayRequest(relayRequest("endpoint_id=ep-1"), {
      getUserId,
      store: store(),
    }),
    400,
  );
});

Deno.test("relay scopes every lookup to the authenticated user", async () => {
  const s = store();
  await expectRelayError(
    authorizeRelayRequest(relayRequest(undefined, "Bearer other-jwt"), {
      getUserId,
      store: s,
    }),
    404,
  );
  assertEquals(s.calls, ["endpoint:user-2:ep-1"]);
});

Deno.test("relay refuses non-managed endpoints", async () => {
  const s = store({
    endpoint: {
      id: "ep-1",
      instance_id: null,
      source: "user_managed",
      hostname: "h",
      port: 22,
    },
  });
  await expectRelayError(
    authorizeRelayRequest(relayRequest(), { getUserId, store: s }),
    400,
  );
});

Deno.test("relay refuses instances that cannot be reached", async () => {
  for (const status of ["provisioning", "failed", "deleted", "deleting"]) {
    const s = store({
      instance: { id: "inst-1", status, provider_resource_id: "x" },
    });
    await expectRelayError(
      authorizeRelayRequest(relayRequest(), { getUserId, store: s }),
      409,
    );
  }
  const noResource = store({
    instance: { id: "inst-1", status: "ready", provider_resource_id: null },
  });
  await expectRelayError(
    authorizeRelayRequest(relayRequest(), { getUserId, store: noResource }),
    409,
  );
});

Deno.test("relay accepts a suspended instance because the proxy wakes it", async () => {
  const s = store({
    instance: { id: "inst-1", status: "suspended", provider_resource_id: "x" },
  });
  const target = await authorizeRelayRequest(relayRequest(), {
    getUserId,
    store: s,
  });
  assertEquals(target.spriteName, "x");
});

Deno.test("relay refuses revoked or foreign client keys", async () => {
  const revoked = store({
    key: { id: "key-1", revoked_at: "2026-01-01T00:00:00Z" },
  });
  await expectRelayError(
    authorizeRelayRequest(relayRequest(), { getUserId, store: revoked }),
    403,
  );
  await expectRelayError(
    authorizeRelayRequest(
      relayRequest("endpoint_id=ep-1&key_id=someone-elses"),
      { getUserId, store: store() },
    ),
    404,
  );
});

Deno.test("relay URL carries ids only and uses a WebSocket scheme", () => {
  Deno.env.set("SUPABASE_URL", "https://proj.supabase.co");
  Deno.env.delete("REMOTE_SSH_RELAY_URL");
  assertEquals(
    relayUrlForEndpoint("ep-1", "key-1"),
    "wss://proj.supabase.co/functions/v1/remote-ssh-relay?endpoint_id=ep-1&key_id=key-1",
  );
  Deno.env.set(
    "REMOTE_SSH_RELAY_URL",
    "http://127.0.0.1:54321/functions/v1/remote-ssh-relay",
  );
  assertEquals(
    relayUrlForEndpoint("ep-1", "key-1"),
    "ws://127.0.0.1:54321/functions/v1/remote-ssh-relay?endpoint_id=ep-1&key_id=key-1",
  );
  Deno.env.delete("REMOTE_SSH_RELAY_URL");
});

Deno.test("sprites proxy URL follows the documented path", () => {
  assertEquals(
    spritesProxyUrl(
      { baseUrl: "https://api.sprites.dev/", apiToken: "t" },
      "dev-treq-a",
    ),
    "wss://api.sprites.dev/v1/sprites/dev-treq-a/proxy",
  );
});

// ---------------------------------------------------------------------------
// Fake sockets
// ---------------------------------------------------------------------------

class FakeSocket implements RelaySocket {
  readyState = 0;
  bufferedAmount = 0;
  binaryType: BinaryType = "blob";
  sent: Array<string | Uint8Array> = [];
  closed: { code?: number; reason?: string } | null = null;
  private listeners = new Map<string, Array<(event: never) => void>>();

  addEventListener(type: string, listener: (event: never) => void): void {
    const list = this.listeners.get(type) ?? [];
    list.push(listener);
    this.listeners.set(type, list);
  }

  emit(type: string, event: unknown = {}): void {
    for (const listener of this.listeners.get(type) ?? [])
      listener(event as never);
  }

  open(): void {
    this.readyState = 1;
    this.emit("open");
  }

  message(data: unknown): void {
    this.emit("message", { data });
  }

  remoteClose(code: number, reason = ""): void {
    this.readyState = 3;
    this.emit("close", { code, reason });
  }

  send(data: ArrayBufferLike | ArrayBufferView | string): void {
    this.sent.push(
      typeof data === "string" ? data : new Uint8Array(data as ArrayBuffer),
    );
  }

  close(code?: number, reason?: string): void {
    if (this.closed) return;
    this.closed = { code, reason };
    this.readyState = 3;
  }
}

const TARGET: RelayTarget = {
  ownerUserId: OWNER,
  endpointId: "ep-1",
  instanceId: "inst-1",
  spriteName: "dev-treq-user-1",
  host: "localhost",
  port: 2222,
};
const CONFIG = { baseUrl: "https://api.sprites.dev", apiToken: "org-token" };

function bytes(...values: number[]): ArrayBuffer {
  return new Uint8Array(values).buffer;
}

Deno.test("upstream handshake sends the target and authenticates with the org token header", async () => {
  const upstream = new FakeSocket();
  let seen: { url: string; headers: Record<string, string> } | null = null;
  const pending = openUpstream(CONFIG, TARGET, (url, headers) => {
    seen = { url, headers };
    return upstream;
  });
  upstream.open();
  assertEquals(upstream.sent, [
    JSON.stringify({ host: "localhost", port: 2222 }),
  ]);
  upstream.message(
    JSON.stringify({ status: "connected", target: "localhost:2222" }),
  );
  const connection = await pending;
  assert(connection.socket === upstream);
  assertEquals(
    seen!.url,
    "wss://api.sprites.dev/v1/sprites/dev-treq-user-1/proxy",
  );
  assertEquals(seen!.headers, { Authorization: "Bearer org-token" });
  assertEquals(upstream.binaryType, "arraybuffer");
});

Deno.test("upstream handshake fails on a refusal or early close", async () => {
  const refused = new FakeSocket();
  const p1 = openUpstream(CONFIG, TARGET, () => refused);
  refused.open();
  refused.message(JSON.stringify({ status: "error" }));
  await assertRejects(() => p1, UpstreamError);

  const dropped = new FakeSocket();
  const p2 = openUpstream(CONFIG, TARGET, () => dropped);
  dropped.remoteClose(1006);
  await assertRejects(() => p2, UpstreamError);
});

Deno.test("upstream handshake times out", async () => {
  const silent = new FakeSocket();
  await assertRejects(
    () => openUpstream(CONFIG, TARGET, () => silent, 10),
    UpstreamError,
  );
  assert(silent.closed);
});

Deno.test("a failed first connect wakes the sprite and retries once", async () => {
  const sockets: FakeSocket[] = [];
  const woke: string[] = [];
  const factory = () => {
    const socket = new FakeSocket();
    sockets.push(socket);
    queueMicrotask(() => {
      socket.open();
      if (sockets.length === 1) socket.remoteClose(1011);
      else socket.message(JSON.stringify({ status: "connected" }));
    });
    return socket;
  };
  const connection = await openUpstreamWithWake(
    CONFIG,
    TARGET,
    factory,
    (name) => {
      woke.push(name);
      return Promise.resolve();
    },
  );
  assertEquals(woke, ["dev-treq-user-1"]);
  assert(connection.socket === sockets[1]);
});

async function connected(): Promise<{
  upstream: FakeSocket;
  connection: Awaited<ReturnType<typeof openUpstream>>;
}> {
  const upstream = new FakeSocket();
  const pending = openUpstream(CONFIG, TARGET, () => upstream);
  upstream.open();
  upstream.message(JSON.stringify({ status: "connected" }));
  return { upstream, connection: await pending };
}

Deno.test("pipe forwards bytes both ways and flushes early upstream bytes once the client opens", async () => {
  const { upstream, connection } = await connected();
  // sshd's banner can arrive before the client socket finishes upgrading.
  upstream.message(bytes(1, 2));
  const client = new FakeSocket();
  const done = pipeSockets(client, connection);
  upstream.message(bytes(3));
  assertEquals(client.sent, []);
  client.open();
  assertEquals(client.sent, [new Uint8Array([1, 2]), new Uint8Array([3])]);

  client.message(bytes(9, 9, 9));
  assertEquals(upstream.sent.slice(1), [new Uint8Array([9, 9, 9])]);

  client.remoteClose(1000);
  const stats = await done;
  assertEquals(stats, {
    bytesFromClient: 3,
    bytesFromUpstream: 3,
    closedBy: "client",
    code: 1000,
  });
  assertEquals(upstream.closed?.code, 1000);
});

Deno.test("pipe propagates an upstream close to the client with a sendable code", async () => {
  const { upstream, connection } = await connected();
  const client = new FakeSocket();
  client.open();
  const done = pipeSockets(client, connection);
  upstream.remoteClose(1006);
  const stats = await done;
  assertEquals(stats.closedBy, "upstream");
  // 1006 is reserved and cannot be sent in a close frame.
  assertEquals(client.closed?.code, 1000);
});

Deno.test("pipe closes both sides when a peer stops reading", async () => {
  const { upstream, connection } = await connected();
  const client = new FakeSocket();
  client.open();
  const done = pipeSockets(client, connection, 4);
  client.bufferedAmount = 3;
  upstream.message(bytes(1, 2));
  const stats = await done;
  assertEquals(stats.closedBy, "overflow");
  assertEquals(client.closed?.code, 1011);
  assertEquals(upstream.closed?.code, 1011);
  assertEquals(client.sent, []);
});

Deno.test("pipe closes both sides on a socket error", async () => {
  const { upstream, connection } = await connected();
  const client = new FakeSocket();
  client.open();
  const done = pipeSockets(client, connection);
  client.emit("error");
  assertEquals((await done).closedBy, "error");
  assert(upstream.closed);
});

Deno.test("host key parsing reads real key lines and skips noise", async () => {
  const line =
    "ssh-ed25519 AAAAC3NzaC1lZDI1NTE5AAAAIOMqqnkVzrm0SdG6UOoqKLsabgH5C9okWi0dh2l9GKJl root@sprite";
  const keys = await parseHostKeyLines(`sudo: warning\n${line}\n\n`);
  assertEquals(keys.length, 1);
  assertEquals(keys[0].algorithm, "ssh-ed25519");
  assert(keys[0].fingerprintSha256.startsWith("SHA256:"));
  assertEquals(await parseHostKeyLines(""), []);
});
