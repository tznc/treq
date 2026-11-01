// The one TCP path from the SSH relay to a machine's sshd, for every
// provider (docs/design/sandbox-provider-adapter.md, D4):
//
//   relay --wss--> provider ingress --> treq-gateway (127.0.0.1:8022) --tcp--> 127.0.0.1:2222
//
// Each adapter only says where its endpoint is
// (`gatewayEndpoint`); the handshake, the gateway token and the
// wake-and-retry live here.

import { GATEWAY_TOKEN_HEADER, mintGatewayTokenFor } from "./gateway-token.ts";
import { ProviderError, type SandboxProvider } from "./provider.ts";
import {
  openProxy,
  type UpstreamConnection,
  UpstreamError,
  type UpstreamSocketFactory,
} from "./gateway-proxy.ts";

/** The placement's gateway secret, loaded by the caller (gateway-secrets.ts). */
export interface GatewayCredential {
  placementId: string;
  secret: string;
}

export interface OpenTcpOptions {
  /** The placement's gateway secret; required for every connect. */
  credential?: GatewayCredential;
  /**
   * Wakes the machine before the single retry. Gateway providers run sshd
   * and the gateway under a pidfile supervisor, so this should also re-run
   * their services (machine-setup.ts `reensureServicesAfterWake`). Defaults
   * to `provider.wakeInstance`.
   */
  wake?: (providerId: string) => Promise<void>;
  timeoutMs?: number;
}

async function attempt(
  provider: SandboxProvider,
  providerId: string,
  host: string,
  port: number,
  factory: UpstreamSocketFactory,
  opts: OpenTcpOptions,
): Promise<UpstreamConnection> {
  // Fresh each attempt: a resumed Boat sandbox may need re-hosting, and the
  // gateway token is short-lived.
  const endpoint = await provider.gatewayEndpoint(providerId);
  const headers = { ...endpoint.headers };
  if (!opts.credential) {
    throw new ProviderError("unavailable", "machine has no gateway secret yet");
  }
  headers[GATEWAY_TOKEN_HEADER] = await mintGatewayTokenFor(
    opts.credential.secret,
    opts.credential.placementId,
  );
  return await openProxy({ url: endpoint.url, headers }, { host, port }, factory, opts.timeoutMs);
}

/**
 * Opens a byte stream to `host:port` inside the machine. If the first
 * attempt fails at the proxy, wakes the machine and tries once more.
 */
export async function openTcp(
  provider: SandboxProvider,
  providerId: string,
  host: string,
  port: number,
  factory: UpstreamSocketFactory,
  opts: OpenTcpOptions = {},
): Promise<UpstreamConnection> {
  try {
    return await attempt(provider, providerId, host, port, factory, opts);
  } catch (err) {
    if (!(err instanceof UpstreamError)) throw err;
    await (opts.wake ?? ((id) => provider.wakeInstance(id)))(providerId);
    return await attempt(provider, providerId, host, port, factory, opts);
  }
}
