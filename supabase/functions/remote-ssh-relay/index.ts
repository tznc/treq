// Edge function: WebSocket relay from a Treq client to sshd on the user's
// managed Sprite (prds/remote-development.md, "SSH identity and
// certificates" > "Managed VM certificate flow").
//
// GET wss://<project>/functions/v1/remote-ssh-relay?endpoint_id=<id>&key_id=<id>
//   Authorization: Bearer <Supabase session JWT>
//
// The relay checks the caller owns the endpoint, the instance can be
// reached, the caller has Pro (402 `pro_required` otherwise), and the client
// key is not revoked. It then opens the Sprites TCP
// proxy with the org token (a server-side secret) and pipes raw bytes. SSH
// runs end to end inside those bytes, so host-key pinning and certificate
// auth are unchanged. See `_shared/remote/ssh-relay.ts` for why a relay
// exists and why its connections are expected to drop after a few minutes.

import { createClient } from "npm:@supabase/supabase-js@2.95.3";
import { userHasPro } from "../_shared/billing/entitlement.ts";
import {
  correlationIdFromRequest,
  logWithCorrelation,
} from "../_shared/remote/correlation.ts";
import {
  authorizeRelayRequest,
  denoUpstreamSocket,
  openUpstreamWithWake,
  pipeSockets,
  RelayError,
  type RelaySocket,
  type RelayStore,
  UpstreamError,
} from "../_shared/remote/ssh-relay.ts";
import {
  SpritesProvider,
  spritesConfigFromEnv,
} from "../_shared/remote/sprites-adapter.ts";

declare const EdgeRuntime:
  | { waitUntil(promise: Promise<unknown>): void }
  | undefined;

function errorResponse(
  message: string,
  status: number,
  correlationId: string,
  code?: string,
): Response {
  return new Response(
    JSON.stringify({
      error: message,
      ...(code ? { code } : {}),
      correlation_id: correlationId,
    }),
    {
      status,
      headers: {
        "Content-Type": "application/json",
        "x-correlation-id": correlationId,
      },
    },
  );
}

Deno.serve(async (req) => {
  const correlationId = correlationIdFromRequest(req);
  if ((req.headers.get("upgrade") ?? "").toLowerCase() !== "websocket") {
    return errorResponse("Expected a WebSocket upgrade", 426, correlationId);
  }

  const supabaseUrl = Deno.env.get("SUPABASE_URL") ?? "";
  const supabaseAnonKey = Deno.env.get("SUPABASE_ANON_KEY") ?? "";
  const supabase = createClient(
    supabaseUrl,
    Deno.env.get("SUPABASE_SERVICE_ROLE_KEY") ?? "",
  );

  const store: RelayStore = {
    async getEndpoint(ownerUserId, endpointId) {
      const { data, error } = await supabase
        .from("remote_endpoints")
        .select("id, instance_id, source, hostname, port")
        .eq("id", endpointId)
        .eq("owner_user_id", ownerUserId)
        .maybeSingle();
      if (error) throw new Error(`failed to read endpoint: ${error.message}`);
      return data;
    },
    async getInstance(ownerUserId, instanceId) {
      const { data, error } = await supabase
        .from("remote_instances")
        .select("id, status, provider_resource_id")
        .eq("id", instanceId)
        .eq("owner_user_id", ownerUserId)
        .maybeSingle();
      if (error) throw new Error(`failed to read instance: ${error.message}`);
      return data;
    },
    async getClientKey(ownerUserId, keyId) {
      const { data, error } = await supabase
        .from("remote_client_keys")
        .select("id, revoked_at")
        .eq("id", keyId)
        .eq("owner_user_id", ownerUserId)
        .maybeSingle();
      if (error) throw new Error(`failed to read client key: ${error.message}`);
      return data;
    },
    hasPro: (ownerUserId) => userHasPro(supabase, ownerUserId),
  };

  let target;
  let upstream;
  try {
    target = await authorizeRelayRequest(req, {
      store,
      getUserId: async (jwt) => {
        const userClient = createClient(supabaseUrl, supabaseAnonKey, {
          global: { headers: { Authorization: `Bearer ${jwt}` } },
        });
        const { data, error } = await userClient.auth.getUser();
        return error || !data.user ? null : data.user.id;
      },
    });
    const config = spritesConfigFromEnv();
    const provider = new SpritesProvider(config);
    upstream = await openUpstreamWithWake(
      config,
      target,
      denoUpstreamSocket,
      (name) => provider.wakeInstance(name),
    );
  } catch (err) {
    if (err instanceof RelayError) {
      logWithCorrelation(
        correlationId,
        "warn",
        `relay refused: status=${err.status} reason=${err.message}`,
      );
      return errorResponse(err.message, err.status, correlationId, err.code);
    }
    if (err instanceof UpstreamError) {
      logWithCorrelation(
        correlationId,
        "warn",
        `relay upstream failed: ${err.message}`,
      );
      return errorResponse(
        "Could not reach the managed VM",
        502,
        correlationId,
      );
    }
    logWithCorrelation(
      correlationId,
      "error",
      `relay failed: ${(err as Error).message}`,
    );
    return errorResponse("Internal error", 500, correlationId);
  }

  const { socket, response } = Deno.upgradeWebSocket(req);
  const started = Date.now();
  logWithCorrelation(
    correlationId,
    "log",
    `relay open endpoint_id=${target.endpointId} instance_id=${target.instanceId}`,
  );
  const done = pipeSockets(socket as unknown as RelaySocket, upstream).then(
    (stats) => {
      // Counts only. Payload bytes are SSH ciphertext and are never logged.
      logWithCorrelation(
        correlationId,
        "log",
        `relay closed endpoint_id=${target.endpointId} by=${stats.closedBy} code=${stats.code} ` +
          `bytes_in=${stats.bytesFromClient} bytes_out=${stats.bytesFromUpstream} duration_ms=${Date.now() - started}`,
      );
    },
  );
  // Keeps the worker alive while the socket is open. The platform wall-clock
  // limit still applies and ends the connection; clients reconnect.
  if (typeof EdgeRuntime !== "undefined") EdgeRuntime.waitUntil(done);
  return response;
});
