// Core of the `remote-ssh-relay` Edge Function: authorizes a client's
// WebSocket upgrade and pipes raw SSH bytes between that socket and the
// Sprites TCP proxy. Kept free of `Deno.serve` and Supabase clients so the
// authorization rules and the byte pump can be unit tested with fakes.
//
// Why a relay: a Sprite has no raw TCP ingress, and the Sprites TCP proxy
// needs the org API token, which must never leave the server. The relay
// holds that token; the client only proves who it is with its Supabase
// session. SSH runs end to end through the relay, so the relay never sees
// plaintext: the client still pins the host key and presents a Treq CA
// certificate that sshd checks.
//
// Lifetime: Supabase Edge Functions cap how long a worker lives (wall clock
// 150 s on Free, 400 s on paid plans at the time of writing; see
// supabase.com/docs/guides/functions/limits), and that cap includes an open
// WebSocket. A relay connection therefore drops after a few minutes at
// most. That is expected: the client's SSH pool reconnects on the next
// command, and PTY sessions run inside tmux on the VM so they reattach.

import {
  PRO_REQUIRED_CODE,
  PRO_REQUIRED_MESSAGES,
} from "../billing/entitlement.ts";
import { spritesProxyUrl, type SpritesConfig } from "./sprites-adapter.ts";

// Instance states in which the Sprite exists and the proxy can reach it. A
// paused Sprite is woken by the relay (see `openUpstreamWithWake`), so
// `suspended` and `waking` are accepted as well as `ready`.
const RELAYABLE_INSTANCE_STATES = new Set(["ready", "suspended", "waking"]);

// How long to wait for the proxy's `{"status":"connected"}` reply. A cold
// wake takes 1 to 2 s per the Sprites lifecycle docs, so this leaves room
// for a wake plus sshd accepting the TCP connection.
const UPSTREAM_CONNECT_TIMEOUT_MS = 15_000;

// Bytes allowed to queue on one side before the relay gives up on that
// connection. WebSocket has no way to pause reads, so this is a bound, not
// flow control. SSH's own per-channel windows (about 2 MiB in russh and
// OpenSSH) keep the in-flight data far below this in normal use, so hitting
// it means the peer stopped reading.
export const RELAY_MAX_BUFFERED_BYTES = 8 * 1024 * 1024;

export interface RelayTarget {
  ownerUserId: string;
  endpointId: string;
  instanceId: string;
  spriteName: string;
  host: string;
  port: number;
}

export interface RelayEndpointRow {
  id: string;
  instance_id: string | null;
  source: string;
  hostname: string;
  port: number;
}

export interface RelayInstanceRow {
  id: string;
  status: string;
  provider_resource_id: string | null;
}

export interface RelayClientKeyRow {
  id: string;
  revoked_at: string | null;
}

// Every lookup is scoped to the authenticated owner. The relay uses the
// service role, which bypasses RLS, so the owner filter is the ownership
// check.
export interface RelayStore {
  getEndpoint(
    ownerUserId: string,
    endpointId: string,
  ): Promise<RelayEndpointRow | null>;
  getInstance(
    ownerUserId: string,
    instanceId: string,
  ): Promise<RelayInstanceRow | null>;
  getClientKey(
    ownerUserId: string,
    keyId: string,
  ): Promise<RelayClientKeyRow | null>;
  /** has_pro for the owner. */
  hasPro(ownerUserId: string): Promise<boolean>;
}

export interface RelayAuthDeps {
  /** Resolves the Supabase user id for a session JWT, or null when invalid. */
  getUserId(jwt: string): Promise<string | null>;
  store: RelayStore;
}

export class RelayError extends Error {
  constructor(
    message: string,
    public readonly status: number,
    /** Machine-readable reason, e.g. `pro_required`. */
    public readonly code?: string,
  ) {
    super(message);
  }
}

// The Supabase JWT travels in the Authorization header of the upgrade
// request, never in the URL, so it does not end up in proxy or access logs.
// Native clients (the Rust transport) can set headers on a WebSocket
// handshake; browsers cannot, but no browser client uses this relay.
export async function authorizeRelayRequest(
  req: Request,
  deps: RelayAuthDeps,
): Promise<RelayTarget> {
  const authHeader = req.headers.get("authorization") ?? "";
  const jwt = authHeader.replace(/^Bearer\s+/i, "");
  if (!jwt) throw new RelayError("Unauthorized", 401);

  const url = new URL(req.url);
  const endpointId = url.searchParams.get("endpoint_id");
  const keyId = url.searchParams.get("key_id");
  if (!endpointId || !keyId)
    throw new RelayError("endpoint_id and key_id are required", 400);

  const ownerUserId = await deps.getUserId(jwt);
  if (!ownerUserId) throw new RelayError("Unauthorized", 401);

  const endpoint = await deps.store.getEndpoint(ownerUserId, endpointId);
  if (!endpoint)
    throw new RelayError("Endpoint does not belong to this user", 404);
  if (endpoint.source !== "managed" || !endpoint.instance_id) {
    throw new RelayError("Only managed endpoints are relayed", 400);
  }

  const instance = await deps.store.getInstance(
    ownerUserId,
    endpoint.instance_id,
  );
  if (!instance)
    throw new RelayError("Instance does not belong to this user", 404);
  if (
    !RELAYABLE_INSTANCE_STATES.has(instance.status) ||
    !instance.provider_resource_id
  ) {
    throw new RelayError(
      `Instance is not ready (status: ${instance.status})`,
      409,
    );
  }

  // Connecting wakes a paused Sprite, so this is what keeps the cloud
  // workspace of an owner whose Pro ended stopped (prds/billing-and-teams.md,
  // "Enforcement").
  if (!(await deps.store.hasPro(ownerUserId))) {
    throw new RelayError(
      PRO_REQUIRED_MESSAGES.cloudWorkspace,
      402,
      PRO_REQUIRED_CODE,
    );
  }

  // sshd checks the certificate, but a revoked key should lose the relay
  // too, so a stolen certificate cannot outlive the revocation by reaching
  // sshd until it expires.
  const key = await deps.store.getClientKey(ownerUserId, keyId);
  if (!key) throw new RelayError("Key does not belong to this user", 404);
  if (key.revoked_at) throw new RelayError("Key has been revoked", 403);

  return {
    ownerUserId,
    endpointId: endpoint.id,
    instanceId: instance.id,
    spriteName: instance.provider_resource_id,
    host: endpoint.hostname,
    port: endpoint.port,
  };
}

/// Public relay URL for an endpoint. `REMOTE_SSH_RELAY_URL` overrides the
/// derived URL; local development needs it because `SUPABASE_URL` inside the
/// functions container is an internal address the client cannot reach.
export function relayUrlForEndpoint(endpointId: string, keyId: string): string {
  const override = Deno.env.get("REMOTE_SSH_RELAY_URL");
  const base =
    override ??
    `${(Deno.env.get("SUPABASE_URL") ?? "").replace(/\/+$/, "")}/functions/v1/remote-ssh-relay`;
  const url = new URL(base);
  if (url.protocol === "http:") url.protocol = "ws:";
  if (url.protocol === "https:") url.protocol = "wss:";
  url.searchParams.set("endpoint_id", endpointId);
  url.searchParams.set("key_id", keyId);
  return url.toString();
}

// ---------------------------------------------------------------------------
// Upstream (Sprites TCP proxy)
// ---------------------------------------------------------------------------

/** The subset of the WebSocket API the relay uses, so tests can pass fakes. */
export interface RelaySocket {
  readonly readyState: number;
  readonly bufferedAmount: number;
  binaryType: BinaryType;
  send(data: ArrayBufferLike | ArrayBufferView | string): void;
  close(code?: number, reason?: string): void;
  addEventListener(type: "open", listener: () => void): void;
  addEventListener(
    type: "message",
    listener: (event: MessageEvent) => void,
  ): void;
  addEventListener(type: "close", listener: (event: CloseEvent) => void): void;
  addEventListener(type: "error", listener: (event: Event) => void): void;
}

export type UpstreamSocketFactory = (
  url: string,
  headers: Record<string, string>,
) => RelaySocket;

// Deno accepts a non-standard `headers` option on the WebSocket client
// constructor, which is how the org token reaches the proxy without being
// placed in the URL.
export const denoUpstreamSocket: UpstreamSocketFactory = (url, headers) =>
  // deno-lint-ignore no-explicit-any
  new WebSocket(url, { headers } as any) as unknown as RelaySocket;

/**
 * An open proxy socket plus any bytes that arrived after the handshake but
 * before the client side was ready. sshd sends its identification line as
 * soon as the TCP connection opens, so those first bytes can arrive early.
 */
export interface UpstreamConnection {
  socket: RelaySocket;
  /** Stops early buffering and returns the bytes buffered so far. */
  takeOver(): Uint8Array[];
}

export class UpstreamError extends Error {}

// Opens the Sprites proxy and completes its JSON handshake. Resolves only
// after `{"status":"connected"}`, so a failure here can still be returned to
// the client as an HTTP error instead of an upgraded socket that closes.
export function openUpstream(
  config: SpritesConfig,
  target: RelayTarget,
  factory: UpstreamSocketFactory,
  timeoutMs = UPSTREAM_CONNECT_TIMEOUT_MS,
): Promise<UpstreamConnection> {
  return new Promise((resolve, reject) => {
    const socket = factory(spritesProxyUrl(config, target.spriteName), {
      Authorization: `Bearer ${config.apiToken}`,
    });
    socket.binaryType = "arraybuffer";
    let pending: Uint8Array[] | null = [];
    let connected = false;
    let settled = false;
    const fail = (message: string) => {
      if (settled) return;
      settled = true;
      clearTimeout(timer);
      try {
        socket.close(1000);
      } catch {
        // Already closed.
      }
      reject(new UpstreamError(message));
    };
    const timer = setTimeout(
      () => fail("timed out waiting for the Sprites proxy"),
      timeoutMs,
    );

    socket.addEventListener("open", () => {
      socket.send(JSON.stringify({ host: target.host, port: target.port }));
    });
    socket.addEventListener("message", (event: MessageEvent) => {
      if (connected) {
        pending?.push(toBytes(event.data));
        return;
      }
      if (typeof event.data !== "string") {
        fail("Sprites proxy sent data before the connect reply");
        return;
      }
      let reply: { status?: unknown };
      try {
        reply = JSON.parse(event.data);
      } catch {
        fail("Sprites proxy sent an unreadable connect reply");
        return;
      }
      if (reply.status !== "connected") {
        fail(
          `Sprites proxy refused the connection (status: ${String(reply.status)})`,
        );
        return;
      }
      connected = true;
      settled = true;
      clearTimeout(timer);
      resolve({
        socket,
        takeOver: () => {
          const early = pending ?? [];
          pending = null;
          return early;
        },
      });
    });
    socket.addEventListener("close", (event: CloseEvent) => {
      fail(`Sprites proxy closed during connect (code ${event.code})`);
    });
    socket.addEventListener("error", () =>
      fail("Sprites proxy connection failed"),
    );
  });
}

// A paused Sprite should resume when the proxy reaches it, but the Sprites
// docs do not say so for the proxy endpoint specifically. If the first
// attempt fails, wake the Sprite through the documented exec path and try
// once more.
export async function openUpstreamWithWake(
  config: SpritesConfig,
  target: RelayTarget,
  factory: UpstreamSocketFactory,
  wake: (spriteName: string) => Promise<void>,
  timeoutMs?: number,
): Promise<UpstreamConnection> {
  try {
    return await openUpstream(config, target, factory, timeoutMs);
  } catch (err) {
    if (!(err instanceof UpstreamError)) throw err;
    await wake(target.spriteName);
    return await openUpstream(config, target, factory, timeoutMs);
  }
}

// ---------------------------------------------------------------------------
// Byte pump
// ---------------------------------------------------------------------------

export interface PipeStats {
  bytesFromClient: number;
  bytesFromUpstream: number;
  closedBy: "client" | "upstream" | "overflow" | "error";
  code: number;
}

function toBytes(data: unknown): Uint8Array {
  if (data instanceof ArrayBuffer) return new Uint8Array(data);
  if (ArrayBuffer.isView(data))
    return new Uint8Array(data.buffer, data.byteOffset, data.byteLength);
  if (typeof data === "string") return new TextEncoder().encode(data);
  return new Uint8Array();
}

// 1005 and 1006 are reserved and may not be sent; 1015 likewise. Anything
// else outside the application range is replaced by a normal close.
function forwardableCloseCode(code: number): number {
  if (code === 1000 || code === 1001 || code === 1011) return code;
  if (code >= 3000 && code <= 4999) return code;
  return code === 1005 || code === 1006 ? 1000 : 1011;
}

const OPEN = 1;

/**
 * Pipes bytes both ways until either side closes, then closes the other
 * with a matching code. Payload bytes are never inspected or logged; only
 * counts are returned. Upstream bytes that arrive before the client socket
 * is open are queued and flushed once it opens.
 */
export function pipeSockets(
  client: RelaySocket,
  upstream: UpstreamConnection,
  maxBuffered = RELAY_MAX_BUFFERED_BYTES,
): Promise<PipeStats> {
  return new Promise((resolve) => {
    const stats: PipeStats = {
      bytesFromClient: 0,
      bytesFromUpstream: 0,
      closedBy: "client",
      code: 1000,
    };
    const server = upstream.socket;
    client.binaryType = "arraybuffer";
    let finished = false;
    let clientOpen = client.readyState === OPEN;
    const queued = upstream.takeOver();

    const finish = (
      closedBy: PipeStats["closedBy"],
      code: number,
      reason = "",
    ) => {
      if (finished) return;
      finished = true;
      stats.closedBy = closedBy;
      stats.code = code;
      const forwarded = forwardableCloseCode(code);
      for (const socket of [client, server]) {
        try {
          socket.close(forwarded, reason);
        } catch {
          // Already closing.
        }
      }
      resolve(stats);
    };

    const deliver = (target: RelaySocket, bytes: Uint8Array): boolean => {
      if (target.bufferedAmount + bytes.byteLength > maxBuffered) {
        finish("overflow", 1011, "relay buffer limit reached");
        return false;
      }
      target.send(bytes);
      return true;
    };

    const flushQueued = () => {
      while (queued.length > 0 && !finished) {
        const bytes = queued.shift()!;
        stats.bytesFromUpstream += bytes.byteLength;
        if (!deliver(client, bytes)) return;
      }
    };

    client.addEventListener("open", () => {
      clientOpen = true;
      flushQueued();
    });
    client.addEventListener("message", (event: MessageEvent) => {
      if (finished) return;
      const bytes = toBytes(event.data);
      stats.bytesFromClient += bytes.byteLength;
      deliver(server, bytes);
    });
    server.addEventListener("message", (event: MessageEvent) => {
      if (finished) return;
      const bytes = toBytes(event.data);
      if (!clientOpen) {
        queued.push(bytes);
        return;
      }
      stats.bytesFromUpstream += bytes.byteLength;
      deliver(client, bytes);
    });
    client.addEventListener("close", (event: CloseEvent) =>
      finish("client", event.code, event.reason),
    );
    server.addEventListener("close", (event: CloseEvent) =>
      finish("upstream", event.code, event.reason),
    );
    client.addEventListener("error", () => finish("error", 1011));
    server.addEventListener("error", () => finish("error", 1011));

    if (clientOpen) flushQueued();
  });
}
