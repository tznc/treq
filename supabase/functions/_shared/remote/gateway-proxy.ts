// Client for the gateway proxy protocol: opens a WebSocket to a machine's
// treq-gateway (through the provider's ingress), sends `{"host","port"}`,
// waits for `{"status":"connected"}`, and hands back a socket that carries
// raw TCP bytes. Used by the shared `openTcp` (provider-tcp.ts), which the
// SSH relay calls. Kept free of the relay so adapters can import it without
// a cycle.

// How long to wait for the gateway's `{"status":"connected"}` reply. Leaves
// room for a machine wake plus sshd accepting the TCP connection.
const UPSTREAM_CONNECT_TIMEOUT_MS = 15_000;

export function toBytes(data: unknown): Uint8Array {
  if (data instanceof ArrayBuffer) return new Uint8Array(data);
  if (ArrayBuffer.isView(data))
    return new Uint8Array(data.buffer, data.byteOffset, data.byteLength);
  if (typeof data === "string") return new TextEncoder().encode(data);
  return new Uint8Array();
}

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
// constructor, which is how provider auth reaches the ingress without being
// placed in the URL.
export const denoUpstreamSocket: UpstreamSocketFactory = (url, headers) =>
  new WebSocket(url, { headers }) as unknown as RelaySocket;

/**
 * An open gateway socket plus any bytes that arrived after the handshake but
 * before the client side was ready. sshd sends its identification line as
 * soon as the TCP connection opens, so those first bytes can arrive early.
 */
export interface UpstreamConnection {
  socket: RelaySocket;
  /** Stops early buffering and returns the bytes buffered so far. */
  takeOver(): Uint8Array[];
}

export class UpstreamError extends Error {}

// Opens the gateway proxy and completes its JSON handshake. Resolves only
// after `{"status":"connected"}`, so a failure here can still be returned to
// the client as an HTTP error instead of an upgraded socket that closes.
export function openProxy(
  endpoint: { url: string; headers: Record<string, string> },
  target: { host: string; port: number },
  factory: UpstreamSocketFactory,
  timeoutMs = UPSTREAM_CONNECT_TIMEOUT_MS,
): Promise<UpstreamConnection> {
  return new Promise((resolve, reject) => {
    const socket = factory(endpoint.url, endpoint.headers);
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
      () => fail("timed out waiting for the gateway proxy"),
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
        fail("gateway proxy sent data before the connect reply");
        return;
      }
      let reply: { status?: unknown };
      try {
        reply = JSON.parse(event.data);
      } catch {
        fail("gateway proxy sent an unreadable connect reply");
        return;
      }
      if (reply.status !== "connected") {
        fail(
          `gateway proxy refused the connection (status: ${String(reply.status)})`,
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
      fail(`gateway proxy closed during connect (code ${event.code})`);
    });
    socket.addEventListener("error", () =>
      fail("gateway proxy connection failed"),
    );
  });
}
