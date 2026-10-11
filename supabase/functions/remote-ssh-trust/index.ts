// Edge function: SSH trust and authentication for Remote Development
// (prds/remote-development.md, Phase 3: "SSH trust and authentication").
//
// POST body: { action, idempotency_key?, ...action-specific fields }
// action:
//   "register_client_key"    - register a user-selected SSH public key (public material only)
//   "list_client_keys"       - list the caller's registered keys
//   "revoke_client_key"      - independently revoke one key
//   "issue_certificate"      - sign a short-lived OpenSSH user certificate for a managed instance
//   "install_authorized_key" - direct-key alternative: install a key into the managed VM's authorized_keys
//   "remove_authorized_key"  - remove a previously installed authorized key
//   "keyscan_endpoint"       - re-run the host-key scan against the caller's managed endpoint
//
// `install_authorized_key` and `keyscan_endpoint` run a command on the
// managed Sprite, which wakes it, so they require Pro like remote-instance's
// `wake` (402 `pro_required`). The other actions only touch the database and
// stay open, so a user whose Pro ended can still revoke keys.
//
// Auth: user JWT in Authorization header. Every action re-derives ownership
// from `owner_user_id` server-side; a caller-supplied instance_id/key_id is
// only ever used to look up a row already scoped to that owner (PRD
// "Security requirements": never trust client-supplied IDs alone).
//
// The SSH CA private key lives only in this function's environment
// (REMOTE_SSH_CA_ED25519_SEED_BASE64) - it is read to sign a certificate and
// never included in any response or database write.

import { createClient, type SupabaseClient } from "npm:@supabase/supabase-js@2.95.3";
import {
  PRO_REQUIRED_MESSAGES,
  refuseGatedAction,
  userHasPro,
} from "../_shared/billing/entitlement.ts";
import { recordAuditEvent, startTimer } from "../_shared/remote/audit.ts";
import { correlationIdFromRequest, logWithCorrelation } from "../_shared/remote/correlation.ts";
import {
  beginOperation,
  completeOperation,
  findExistingOperation,
  getInstanceById,
  type InstanceRow,
} from "../_shared/remote/instance-store.ts";
import {
  findClientKeyByFingerprint,
  getOwnedClientKey,
  insertClientKey,
  listClientKeys,
  recordAuthorizedKeyInstalled,
  recordAuthorizedKeyRemoved,
  revokeClientKey,
  type ClientKeyRow,
} from "../_shared/remote/client-key-store.ts";
import { extractEd25519RawKey, parseOpenSshPublicKey, UnsupportedKeyError } from "../_shared/remote/ssh-keys.ts";
import { caKeyMaterialFromEnv, issueEd25519UserCertificate, randomSerial } from "../_shared/remote/ssh-cert.ts";
import { installAuthorizedKeyCommand, removeAuthorizedKeyCommand } from "../_shared/remote/ssh-vm-config.ts";
import {
  ProviderError,
  SpritesProvider,
  spritesConfigFromEnv,
  type ManagedComputeProvider,
} from "../_shared/remote/sprites-adapter.ts";
import { isSpritesStubEnabled, StubSpritesProvider } from "../_shared/remote/stub-sprites-adapter.ts";
import { KeyscanError } from "../_shared/remote/ssh-keyscan.ts";
import { readManagedHostKeys } from "../_shared/remote/managed-sshd.ts";
import { relayUrlForEndpoint } from "../_shared/remote/ssh-relay.ts";
import { recordEndpointHostKey } from "../_shared/remote/instance-store.ts";

const corsHeaders = {
  "Access-Control-Allow-Origin": "*",
  "Access-Control-Allow-Headers": "authorization, x-client-info, apikey, content-type",
};

// Certificates are deliberately short-lived (PRD "Certificate lifetime
// should be short enough to bound loss exposure while allowing normal
// reconnects") while still tolerating a slow client clock or a long-running
// interactive session started just before expiry.
// Actions that exec on the managed Sprite and so wake it.
const PRO_GATED_TRUST_ACTIONS: ReadonlySet<string> = new Set([
  "install_authorized_key",
  "keyscan_endpoint",
]);

const CERTIFICATE_VALIDITY_MINUTES = 20;
const CERTIFICATE_CLOCK_SKEW_MINUTES = 2;

function json(body: unknown, status = 200, correlationId?: string): Response {
  return new Response(JSON.stringify(correlationId ? { ...(body as object), correlation_id: correlationId } : body), {
    status,
    headers: {
      ...corsHeaders,
      "Content-Type": "application/json",
      ...(correlationId ? { "x-correlation-id": correlationId } : {}),
    },
  });
}

class ValidationError extends Error {
  constructor(message: string, public readonly status = 400) {
    super(message);
  }
}

function getProvider(): ManagedComputeProvider {
  if (isSpritesStubEnabled()) return new StubSpritesProvider();
  return new SpritesProvider(spritesConfigFromEnv());
}

function requireIdempotencyKey(body: Record<string, unknown>): string {
  const key = body.idempotency_key;
  if (typeof key !== "string" || key.length === 0) throw new ValidationError("idempotency_key is required");
  return key;
}

function requireString(body: Record<string, unknown>, field: string): string {
  const value = body[field];
  if (typeof value !== "string" || value.length === 0) throw new ValidationError(`${field} is required`);
  return value;
}

Deno.serve(async (req) => {
  if (req.method === "OPTIONS") return new Response(null, { headers: corsHeaders, status: 204 });
  if (req.method !== "POST") return json({ error: "Method not allowed" }, 405);

  const authHeader = req.headers.get("authorization") ?? "";
  const userToken = authHeader.replace(/^Bearer\s+/i, "");
  if (!userToken) return json({ error: "Unauthorized" }, 401);

  const supabaseUrl = Deno.env.get("SUPABASE_URL") ?? "";
  const supabaseAnonKey = Deno.env.get("SUPABASE_ANON_KEY") ?? "";
  const supabaseServiceKey = Deno.env.get("SUPABASE_SERVICE_ROLE_KEY") ?? "";

  const supabaseUser = createClient(supabaseUrl, supabaseAnonKey, {
    global: { headers: { Authorization: `Bearer ${userToken}` } },
  });
  const {
    data: { user },
    error: authError,
  } = await supabaseUser.auth.getUser();
  if (authError || !user) return json({ error: "Unauthorized" }, 401);

  // deno-lint-ignore no-explicit-any
  let body: Record<string, any>;
  try {
    body = await req.json();
  } catch {
    return json({ error: "Invalid JSON" }, 400);
  }

  const action = body.action;
  const supabase = createClient(supabaseUrl, supabaseServiceKey);
  const correlationId = correlationIdFromRequest(req);

  try {
    const refused = await refuseGatedAction(
      action,
      PRO_GATED_TRUST_ACTIONS,
      () => userHasPro(supabase, user.id),
      PRO_REQUIRED_MESSAGES.cloudWorkspace,
    );
    if (refused) return json(refused.body, refused.status, correlationId);
    switch (action) {
      case "register_client_key":
        return await handleRegisterClientKey(supabase, user.id, body, correlationId);
      case "list_client_keys":
        return await handleListClientKeys(supabase, user.id, correlationId);
      case "revoke_client_key":
        return await handleRevokeClientKey(supabase, user.id, body, correlationId);
      case "issue_certificate":
        return await handleIssueCertificate(supabase, user.id, body, correlationId);
      case "install_authorized_key":
        return await handleAuthorizedKeyChange(supabase, user.id, body, "install", correlationId);
      case "remove_authorized_key":
        return await handleAuthorizedKeyChange(supabase, user.id, body, "remove", correlationId);
      case "keyscan_endpoint":
        return await handleKeyscanEndpoint(supabase, user.id, body, correlationId);
      case "report_cutoff":
        return await handleReportCutoff(supabase, user.id, body, correlationId);
      default:
        return json({ error: `Unknown action '${action}'` }, 400, correlationId);
    }
  } catch (err) {
    if (err instanceof ValidationError) return json({ error: err.message }, err.status, correlationId);
    if (err instanceof UnsupportedKeyError) return json({ error: err.message }, 400, correlationId);
    if (err instanceof ProviderError) return json({ error: err.message, provider_error: err.kind }, 502, correlationId);
    if (err instanceof KeyscanError) return json({ error: err.message, keyscan_error: err.kind }, 502, correlationId);
    logWithCorrelation(correlationId, "error", `remote-ssh-trust action=${action} failed: ${(err as Error).message}`);
    return json({ error: "Internal error" }, 500, correlationId);
  }
});

function toClientKeyResponse(row: ClientKeyRow) {
  return {
    id: row.id,
    algorithm: row.algorithm,
    fingerprint_sha256: row.fingerprint_sha256,
    comment: row.comment,
    created_at: row.created_at,
    revoked_at: row.revoked_at,
  };
}

// Register a user-selected public key. Only public material, its algorithm,
// and its fingerprint are stored (PRD "Client key policy") - the request
// body never carries a private key and none is generated here.
async function handleRegisterClientKey(
  supabase: SupabaseClient,
  ownerUserId: string,
  // deno-lint-ignore no-explicit-any
  body: Record<string, any>,
  correlationId: string,
): Promise<Response> {
  const idempotencyKey = requireIdempotencyKey(body);
  const publicKeyLine = requireString(body, "public_key");
  const comment: string | null = typeof body.comment === "string" ? body.comment : null;

  const existingOp = await findExistingOperation(supabase, ownerUserId, idempotencyKey);
  if (existingOp) {
    const keys = await listClientKeys(supabase, ownerUserId);
    return json({ operation_id: existingOp.id, status: existingOp.status, keys: keys.map(toClientKeyResponse) }, 200, correlationId);
  }

  const parsed = await parseOpenSshPublicKey(publicKeyLine);

  const op = await beginOperation(supabase, {
    ownerUserId,
    instanceId: null,
    operationType: "register_client_key",
    idempotencyKey,
  });

  const existing = await findClientKeyByFingerprint(supabase, ownerUserId, parsed.fingerprintSha256);
  if (existing && !existing.revoked_at) {
    await completeOperation(supabase, op.id, { status: "succeeded" });
    return json({ operation_id: op.id, status: "succeeded", key: toClientKeyResponse(existing) }, 200, correlationId);
  }

  const row = await insertClientKey(supabase, {
    ownerUserId,
    publicKey: publicKeyLine,
    algorithm: parsed.algorithm,
    fingerprintSha256: parsed.fingerprintSha256,
    comment: comment ?? parsed.comment,
  });
  await completeOperation(supabase, op.id, { status: "succeeded" });
  await recordAuditEvent(supabase, {
    ownerUserId,
    eventType: "client_key_registered",
    detail: { key_id: row.id, algorithm: row.algorithm, fingerprint: row.fingerprint_sha256 },
    correlationId,
    idempotencyKey,
  });
  return json({ operation_id: op.id, status: "succeeded", key: toClientKeyResponse(row) }, 200, correlationId);
}

async function handleListClientKeys(supabase: SupabaseClient, ownerUserId: string, correlationId: string): Promise<Response> {
  const keys = await listClientKeys(supabase, ownerUserId);
  return json({ keys: keys.map(toClientKeyResponse) }, 200, correlationId);
}

// Each client key is independently revocable (PRD Goal 6/7). Revoking does
// not remove any authorized_keys installs already made with it - callers
// that also want the VM entry removed should call remove_authorized_key.
async function handleRevokeClientKey(
  supabase: SupabaseClient,
  ownerUserId: string,
  // deno-lint-ignore no-explicit-any
  body: Record<string, any>,
  correlationId: string,
): Promise<Response> {
  const idempotencyKey = requireIdempotencyKey(body);
  const keyId = requireString(body, "key_id");

  const existingOp = await findExistingOperation(supabase, ownerUserId, idempotencyKey);
  if (existingOp) return json({ operation_id: existingOp.id, status: existingOp.status }, 200, correlationId);

  const key = await getOwnedClientKey(supabase, ownerUserId, keyId);
  if (!key) throw new ValidationError("Key does not belong to this user", 404);

  const op = await beginOperation(supabase, {
    ownerUserId,
    instanceId: null,
    operationType: "revoke_client_key",
    idempotencyKey,
  });
  await revokeClientKey(supabase, ownerUserId, keyId);
  await completeOperation(supabase, op.id, { status: "succeeded" });
  await recordAuditEvent(supabase, {
    ownerUserId,
    eventType: "client_key_revoked",
    detail: { key_id: keyId, fingerprint: key.fingerprint_sha256 },
    correlationId,
    idempotencyKey,
  });
  return json({ operation_id: op.id, status: "succeeded" }, 200, correlationId);
}

// Resolves and verifies an owned, ready managed instance with its endpoint,
// per the "verifies ownership, key status, and instance status" step of the
// PRD's certificate flow.
async function requireReadyOwnedInstance(
  supabase: SupabaseClient,
  ownerUserId: string,
  instanceId: string,
): Promise<InstanceRow> {
  const instance = await getInstanceById(supabase, ownerUserId, instanceId);
  if (!instance) throw new ValidationError("Instance does not belong to this user", 404);
  if (instance.status !== "ready") {
    throw new ValidationError(`Instance is not ready (status: ${instance.status})`, 409);
  }
  if (!instance.endpoint_id) throw new ValidationError("Instance has no endpoint recorded yet", 409);
  return instance;
}

async function requireActiveOwnedKey(
  supabase: SupabaseClient,
  ownerUserId: string,
  keyId: string,
): Promise<ClientKeyRow> {
  const key = await getOwnedClientKey(supabase, ownerUserId, keyId);
  if (!key) throw new ValidationError("Key does not belong to this user", 404);
  if (key.revoked_at) throw new ValidationError("Key has been revoked", 409);
  return key;
}

// Signs a short-lived OpenSSH user certificate for a managed instance (PRD
// "Managed VM certificate flow", steps 4-5). The CA private key is read from
// an Edge Function secret for the duration of this call only.
async function handleIssueCertificate(
  supabase: SupabaseClient,
  ownerUserId: string,
  // deno-lint-ignore no-explicit-any
  body: Record<string, any>,
  correlationId: string,
): Promise<Response> {
  const elapsed = startTimer();
  const instanceId = requireString(body, "instance_id");
  const keyId = requireString(body, "key_id");
  // The desktop client's silent-renewal loop (PRD "Silent renewal while the
  // session is active") calls this same action again ahead of expiry rather
  // than a separate endpoint - it sets `renewal: true` purely so the audit
  // trail records a renewal distinctly from first issuance ("certificate
  // serial, principals, issue time, and expiry... each renewal").
  const isRenewal = body.renewal === true;

  const instance = await requireReadyOwnedInstance(supabase, ownerUserId, instanceId);
  const key = await requireActiveOwnedKey(supabase, ownerUserId, keyId);

  if (key.algorithm !== "ssh-ed25519") {
    // Real gap, not a stub: only ed25519 user keys are certified today (see
    // ssh-cert.ts doc comment). RSA/ECDSA certificate signing is future work.
    throw new ValidationError(
      `Certificate signing is only implemented for ssh-ed25519 keys (this key is ${key.algorithm})`,
      400,
    );
  }

  const { data: endpointRow, error: endpointError } = await supabase
    .from("remote_endpoints")
    .select("id, hostname, port, username, source, instance_id")
    .eq("id", instance.endpoint_id)
    .eq("owner_user_id", ownerUserId)
    .maybeSingle();
  if (endpointError) throw new Error(`failed to read endpoint: ${endpointError.message}`);
  if (!endpointRow) throw new ValidationError("Endpoint does not belong to this user", 404);

  const { data: hostKeyRows, error: hostKeyError } = await supabase
    .from("remote_endpoint_host_keys")
    .select("algorithm, fingerprint_sha256, comment")
    .eq("endpoint_id", endpointRow.id)
    // The service role bypasses RLS, so filter on owner here too. A host
    // key owned by anyone else must never be served as trusted for this
    // user's endpoint, even if such a row were written by mistake.
    .eq("owner_user_id", ownerUserId)
    .is("revoked_at", null)
    .order("generation", { ascending: false });
  if (hostKeyError) throw new Error(`failed to read host keys: ${hostKeyError.message}`);

  const parsedUserKey = await parseOpenSshPublicKey(key.public_key);
  const rawUserKey = extractEd25519RawKey(parsedUserKey);

  const ca = caKeyMaterialFromEnv();
  const now = new Date();
  const validAfter = new Date(now.getTime() - CERTIFICATE_CLOCK_SKEW_MINUTES * 60_000);
  const validBefore = new Date(now.getTime() + CERTIFICATE_VALIDITY_MINUTES * 60_000);
  const serial = randomSerial();

  let issued;
  try {
    issued = await issueEd25519UserCertificate({
      ca,
      userPublicKey: rawUserKey,
      principals: [endpointRow.username, instance.id],
      serial,
      keyId: `treq:${ownerUserId}:${instance.id}`,
      validAfter,
      validBefore,
    });
  } catch (err) {
    await recordAuditEvent(supabase, {
      ownerUserId,
      instanceId: instance.id,
      endpointId: endpointRow.id,
      eventType: isRenewal ? "certificate_renewal_failed" : "certificate_issue_failed",
      detail: { error: (err as Error).message },
      correlationId,
      durationMs: elapsed(),
    });
    throw err;
  }

  await recordAuditEvent(supabase, {
    ownerUserId,
    instanceId: instance.id,
    endpointId: endpointRow.id,
    eventType: isRenewal ? "certificate_renewed" : "certificate_issued",
    detail: {
      serial: issued.serial,
      principals: [endpointRow.username, instance.id],
      issued_at: validAfter.toISOString(),
      expires_at: validBefore.toISOString(),
      key_id: key.id,
    },
    correlationId,
    durationMs: elapsed(),
  });

  return json({
    certificate: issued.certificateLine,
    serial: issued.serial,
    expires_at: validBefore.toISOString(),
    endpoint: {
      id: endpointRow.id,
      instance_id: endpointRow.instance_id,
      source: { type: "managed", provider: "fly_sprites", generation: instance.generation },
      hostname: endpointRow.hostname,
      port: endpointRow.port,
      username: endpointRow.username,
      host_keys: (hostKeyRows ?? []).map((row: { algorithm: string; fingerprint_sha256: string; comment: string | null }) => ({
        algorithm: row.algorithm,
        fingerprint_sha256: row.fingerprint_sha256,
        comment: row.comment,
      })),
      authentication: { type: "certificate", key_reference: key.id },
      // Sprites expose no raw TCP ingress, so managed endpoints are reached
      // through the `remote-ssh-relay` Edge Function. `hostname`/`port`
      // above name sshd as seen from inside the Sprite, not a dialable
      // address. The URL carries only ids; the client sends its session JWT
      // in a header on each connection.
      transport: { type: "relay", url: relayUrlForEndpoint(endpointRow.id, key.id) },
    },
  }, 200, correlationId);
}

// Direct existing-key auth alternative (PRD "Existing keys without
// certificates"): idempotently installs or removes a registered public key
// in the managed VM's authorized_keys via the provider's exec channel.
async function handleAuthorizedKeyChange(
  supabase: SupabaseClient,
  ownerUserId: string,
  // deno-lint-ignore no-explicit-any
  body: Record<string, any>,
  mode: "install" | "remove",
  correlationId: string,
): Promise<Response> {
  const idempotencyKey = requireIdempotencyKey(body);
  const instanceId = requireString(body, "instance_id");
  const keyId = requireString(body, "key_id");

  const existingOp = await findExistingOperation(supabase, ownerUserId, idempotencyKey);
  if (existingOp) return json({ operation_id: existingOp.id, status: existingOp.status }, 200, correlationId);

  const instance = await requireReadyOwnedInstance(supabase, ownerUserId, instanceId);
  const key = await getOwnedClientKey(supabase, ownerUserId, keyId);
  if (!key) throw new ValidationError("Key does not belong to this user", 404);
  if (mode === "install" && key.revoked_at) throw new ValidationError("Key has been revoked", 409);
  if (!instance.provider_resource_id) throw new ValidationError("Instance has no provider resource yet", 409);

  const op = await beginOperation(supabase, {
    ownerUserId,
    instanceId: instance.id,
    operationType: mode === "install" ? "install_authorized_key" : "remove_authorized_key",
    idempotencyKey,
  });

  try {
    const provider = getProvider();
    const command =
      mode === "install"
        ? installAuthorizedKeyCommand(key.public_key, key.fingerprint_sha256)
        : removeAuthorizedKeyCommand(key.fingerprint_sha256);
    const result = await provider.execOnMachine(instance.provider_resource_id, command);
    if (result.exitCode !== 0) {
      throw new Error(`authorized_keys ${mode} exited ${result.exitCode}: ${result.stderr || result.stdout}`);
    }

    if (mode === "install") {
      await recordAuthorizedKeyInstalled(supabase, {
        ownerUserId,
        endpointId: instance.endpoint_id!,
        clientKeyId: key.id,
      });
    } else {
      await recordAuthorizedKeyRemoved(supabase, instance.endpoint_id!, key.id);
    }

    await completeOperation(supabase, op.id, { status: "succeeded" });
    await recordAuditEvent(supabase, {
      ownerUserId,
      instanceId: instance.id,
      endpointId: instance.endpoint_id,
      eventType: mode === "install" ? "authorized_key_installed" : "authorized_key_removed",
      detail: { key_id: key.id, fingerprint: key.fingerprint_sha256 },
      correlationId,
      idempotencyKey,
    });
    return json({ operation_id: op.id, status: "succeeded" }, 200, correlationId);
  } catch (err) {
    await completeOperation(supabase, op.id, { status: "failed", errorMessage: (err as Error).message });
    throw err;
  }
}

// Re-reads the managed VM's sshd host key over the provider's exec API and
// records old/new fingerprint, generation, and provider resource id as a
// rotation record (PRD "Reprovisioning may rotate the host key"). The key is
// read on the machine rather than scanned over the network because sshd
// only listens on the Sprite's loopback interface.
async function handleKeyscanEndpoint(
  supabase: SupabaseClient,
  ownerUserId: string,
  // deno-lint-ignore no-explicit-any
  body: Record<string, any>,
  correlationId: string,
): Promise<Response> {
  const instanceId = requireString(body, "instance_id");
  const instance = await getInstanceById(supabase, ownerUserId, instanceId);
  if (!instance) throw new ValidationError("Instance does not belong to this user", 404);
  if (!instance.endpoint_id) throw new ValidationError("Instance has no endpoint recorded yet", 409);
  if (!instance.provider_resource_id) throw new ValidationError("Instance has no provider resource yet", 409);

  const { data: endpointRow, error: endpointError } = await supabase
    .from("remote_endpoints")
    .select("id")
    .eq("id", instance.endpoint_id)
    .eq("owner_user_id", ownerUserId)
    .maybeSingle();
  if (endpointError) throw new Error(`failed to read endpoint: ${endpointError.message}`);
  if (!endpointRow) throw new ValidationError("Endpoint does not belong to this user", 404);

  try {
    const hostKeys = await readManagedHostKeys(getProvider(), instance.provider_resource_id);
    const scanned = hostKeys[0];
    if (!scanned) throw new KeyscanError("sshd host key file held no ed25519 key", "unsupported_host_key_algorithm");
    await recordEndpointHostKey(supabase, {
      ownerUserId,
      endpointId: endpointRow.id,
      algorithm: scanned.algorithm,
      fingerprintSha256: scanned.fingerprintSha256,
      generation: instance.generation,
    });
    await recordAuditEvent(supabase, {
      ownerUserId,
      instanceId: instance.id,
      endpointId: endpointRow.id,
      eventType: "host_key_registered",
      detail: { algorithm: scanned.algorithm, fingerprint: scanned.fingerprintSha256, generation: instance.generation },
      correlationId,
    });
    return json({ host_key: { algorithm: scanned.algorithm, fingerprint_sha256: scanned.fingerprintSha256 } }, 200, correlationId);
  } catch (err) {
    await recordAuditEvent(supabase, {
      ownerUserId,
      instanceId: instance.id,
      endpointId: endpointRow.id,
      eventType: "host_keyscan_failed",
      detail: { error: (err as Error).message },
      correlationId,
    });
    throw err;
  }
}

// Records that the desktop client forced a hard cutoff for an endpoint (PRD
// "Hard cutoff on revocation or expiry"). The cutoff itself already
// happened client-side (open channels torn down, further commands refused)
// before this call is made - this is purely so "forced cutoffs from key
// revocation or certificate lapse" are correlatable in the same audit trail
// as the revocation or lapse that caused them. Best-effort: the client does
// not block its own cutoff on this call succeeding, so failures here are
// swallowed by the caller rather than surfaced to the user.
async function handleReportCutoff(
  supabase: SupabaseClient,
  ownerUserId: string,
  // deno-lint-ignore no-explicit-any
  body: Record<string, any>,
  correlationId: string,
): Promise<Response> {
  const reason = requireString(body, "reason");
  const instanceId = typeof body.instance_id === "string" ? body.instance_id : null;
  const endpointId = typeof body.endpoint_id === "string" ? body.endpoint_id : null;

  await recordAuditEvent(supabase, {
    ownerUserId,
    instanceId,
    endpointId,
    eventType: "certificate_cutoff_forced",
    detail: { reason },
    correlationId,
    severity: "warning",
  });
  return json({ status: "recorded" }, 200, correlationId);
}
