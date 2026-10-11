// Request logic for the remote-instance Edge Function (see index.ts for the
// actions), free of Deno globals so it runs under service-qa. index.ts
// verifies the user's JWT and supplies the service-role client, the
// provider, and the user's Pro check.
//
// `ensure`, `wake` and `reprovision` require Pro (prds/billing-and-teams.md,
// "Enforcement"). `status` and `delete` always work, so a user whose Pro
// ended can still see their cloud workspace and remove it.

import type { SupabaseClient } from "npm:@supabase/supabase-js@2.95.3";
import {
  PRO_REQUIRED_MESSAGES,
  refuseGatedAction,
} from "../_shared/billing/entitlement.ts";
import {
  isRegionCode,
  isSizePreset,
  REGION_CODES,
  SIZE_PRESETS,
  type RegionCode,
  type SizePreset,
} from "../_shared/remote/catalog.ts";
import {
  bootstrapCommand,
  CURRENT_MANIFEST_VERSION,
} from "../_shared/remote/boot-manifest.ts";
import {
  ProviderError,
  type ManagedComputeProvider,
} from "../_shared/remote/sprites-adapter.ts";
import { recordAuditEvent, startTimer } from "../_shared/remote/audit.ts";
import { logWithCorrelation } from "../_shared/remote/correlation.ts";
import {
  beginOperation,
  completeOperation,
  createProvisioningInstance,
  findExistingOperation,
  getInstanceForOwner,
  previousHostKeyFingerprint,
  recordEndpointHostKey,
  recordManagedEndpoint,
  updateInstance,
  type InstanceRow,
} from "../_shared/remote/instance-store.ts";
import {
  provisionManagedSshd,
  readManagedHostKeys,
} from "../_shared/remote/managed-sshd.ts";
import {
  caKeyMaterialFromEnv,
  caPublicKeyLine,
} from "../_shared/remote/ssh-cert.ts";
import {
  MANAGED_SSHD_HOST,
  MANAGED_SSHD_PORT,
} from "../_shared/remote/ssh-vm-config.ts";

export const corsHeaders = {
  "Access-Control-Allow-Origin": "*",
  "Access-Control-Allow-Headers":
    "authorization, x-client-info, apikey, content-type",
};

const MANAGED_SSH_USERNAME = "treq";

export function json(body: unknown, status = 200, correlationId?: string): Response {
  return new Response(
    JSON.stringify(
      correlationId
        ? { ...(body as object), correlation_id: correlationId }
        : body,
    ),
    {
      status,
      headers: {
        ...corsHeaders,
        "Content-Type": "application/json",
        ...(correlationId ? { "x-correlation-id": correlationId } : {}),
      },
    },
  );
}

// Clients (desktop's ManagedInstanceRecord in remote_provider.rs, mobile's
// ManagedInstanceRecord in api-types-remote.ts) deserialize this instance
// under an `instance_id` field, but the DB row's primary key column is
// `id` - serialize it under the wire contract's name rather than the raw
// row, or every instance-bearing response silently fails to round-trip
// `instance_id` on both clients.
function serializeInstance(
  row: InstanceRow | null,
): (Omit<InstanceRow, "id"> & { instance_id: string }) | null {
  if (!row) return null;
  const { id, ...rest } = row;
  return { instance_id: id, ...rest };
}

/** Where the handlers get the compute provider. */
export interface ProviderEnv {
  provider: () => ManagedComputeProvider;
  /** The in-memory stub: there is no machine to exec on. */
  stub: boolean;
}

export interface RemoteInstanceDeps extends ProviderEnv {
  /** Service-role client. */
  supabase: SupabaseClient;
  ownerUserId: string;
  correlationId: string;
  /** has_pro for the caller. */
  hasPro: () => Promise<boolean>;
}

/** Actions that start or repair a machine. The rest stay open to Free users. */
export const PRO_GATED_ACTIONS: ReadonlySet<string> = new Set([
  "ensure",
  "wake",
  "reprovision",
]);

function providerErrorStatus(err: ProviderError): number {
  switch (err.kind) {
    case "not_found":
      return 404;
    case "already_exists":
      return 409;
    case "quota_exceeded":
      return 429;
    case "invalid_request":
      return 400;
    case "timeout":
      return 504;
    case "unavailable":
      return 502;
    default:
      return 500;
  }
}

export async function handleRemoteInstanceAction(
  // deno-lint-ignore no-explicit-any
  body: Record<string, any>,
  deps: RemoteInstanceDeps,
): Promise<Response> {
  const action = body.action;
  const { supabase, ownerUserId, correlationId } = deps;

  try {
    const refused = await refuseGatedAction(
      action,
      PRO_GATED_ACTIONS,
      deps.hasPro,
      PRO_REQUIRED_MESSAGES.cloudWorkspace,
    );
    if (refused) return json(refused.body, refused.status, correlationId);
    switch (action) {
      case "list_regions":
        return json({ regions: REGION_CODES }, 200, correlationId);
      case "list_sizes":
        return json({ presets: SIZE_PRESETS }, 200, correlationId);
      case "status":
        return await handleStatus(supabase, ownerUserId, correlationId, deps);
      case "ensure":
        return await handleEnsure(supabase, ownerUserId, body, correlationId, deps);
      case "wake":
        return await handleWake(supabase, ownerUserId, body, correlationId, deps);
      case "reprovision":
        return await handleReprovision(supabase, ownerUserId, body, correlationId, deps);
      case "delete":
        return await handleDelete(supabase, ownerUserId, body, correlationId, deps);
      default:
        return json(
          { error: `Unknown action '${action}'` },
          400,
          correlationId,
        );
    }
  } catch (err) {
    if (err instanceof ProviderError) {
      return json(
        { error: err.message, code: err.kind, provider_error: err.kind },
        providerErrorStatus(err),
        correlationId,
      );
    }
    if (err instanceof ValidationErrorWithStatus) {
      return json({ error: err.message, code: "validation_error" }, err.status, correlationId);
    }
    logWithCorrelation(
      correlationId,
      "error",
      `remote-instance action=${action} failed: ${(err as Error).message}`,
    );
    return json({ error: "Internal error", code: "internal_error" }, 500, correlationId);
  }
}

async function handleStatus(
  supabase: SupabaseClient,
  ownerUserId: string,
  correlationId: string,
  env: ProviderEnv,
): Promise<Response> {
  let instance = await getInstanceForOwner(supabase, ownerUserId);
  if (!instance)
    return json({ instance: null, endpoint: null }, 200, correlationId);

  if (instance.provider_resource_id && instance.status !== "deleted") {
    try {
      const observed = await env.provider().getInstance(
        instance.provider_resource_id,
      );
      const status = mapProviderStateToInstanceStatus(observed.state);
      await updateInstance(supabase, instance.id, {
        status,
        ready_at:
          status === "ready"
            ? (instance.ready_at ?? new Date().toISOString())
            : instance.ready_at,
      });
      instance = (await getInstanceForOwner(supabase, ownerUserId)) ?? instance;
    } catch (err) {
      logWithCorrelation(
        correlationId,
        "warn",
        `could not refresh Sprite status: ${(err as Error).message}`,
      );
    }
  }

  let endpoint = null;
  if (instance.endpoint_id) {
    const { data } = await supabase
      .from("remote_endpoints")
      .select("id, hostname, port, username, source")
      .eq("id", instance.endpoint_id)
      .maybeSingle();
    endpoint = data ?? null;
  }
  return json(
    { instance: serializeInstance(instance), endpoint },
    200,
    correlationId,
  );
}

// Brings up the loopback sshd on the freshly (re)provisioned Sprite, trusts
// the Treq SSH CA (PRD "Configure the managed VM to trust the Treq SSH
// CA"), and records the real host key read over the provider's exec API.
// A Sprite has no raw TCP ingress, so there is nothing to keyscan from here:
// clients reach this sshd only through the `remote-ssh-relay` Edge Function.
// A failure is recorded as an auditable readiness-stage failure rather than
// failing the whole provision/reprovision operation, so a later repair or
// explicit `keyscan_endpoint` can complete it.
async function establishSshTrust(
  supabase: SupabaseClient,
  provider: ManagedComputeProvider,
  params: {
    ownerUserId: string;
    instanceId: string;
    endpointId: string;
    providerResourceId: string;
    generation: number;
    correlationId: string;
    stub: boolean;
  },
): Promise<void> {
  if (params.stub) {
    // The stub adapter has no real machine: there is nothing to exec
    // against in local/service-qa mode. Record a clearly-labeled stub
    // fingerprint so downstream code paths that expect a host key row still
    // have one.
    await recordEndpointHostKey(supabase, {
      ownerUserId: params.ownerUserId,
      endpointId: params.endpointId,
      algorithm: "ssh-ed25519",
      fingerprintSha256: "SHA256:stub-mode-no-real-host-key",
      generation: params.generation,
    });
    await recordAuditEvent(supabase, {
      ownerUserId: params.ownerUserId,
      instanceId: params.instanceId,
      endpointId: params.endpointId,
      eventType: "host_key_registered",
      detail: {
        note: "TREQ_REMOTE_SPRITES_STUB active: recorded a placeholder fingerprint, not a real host key",
        generation: params.generation,
      },
      correlationId: params.correlationId,
    });
    return;
  }

  try {
    await provisionManagedSshd(
      provider,
      params.providerResourceId,
      caPublicKeyLine(caKeyMaterialFromEnv()),
    );
    await recordAuditEvent(supabase, {
      ownerUserId: params.ownerUserId,
      instanceId: params.instanceId,
      endpointId: params.endpointId,
      eventType: "ca_trust_installed",
      detail: { generation: params.generation },
      correlationId: params.correlationId,
    });
  } catch (err) {
    await recordAuditEvent(supabase, {
      ownerUserId: params.ownerUserId,
      instanceId: params.instanceId,
      endpointId: params.endpointId,
      eventType: "ca_trust_install_failed",
      detail: { error: (err as Error).message },
      correlationId: params.correlationId,
    });
    return;
  }

  try {
    const hostKeys = await readManagedHostKeys(
      provider,
      params.providerResourceId,
    );
    if (hostKeys.length === 0) {
      throw new Error("sshd host key file held no ed25519 key");
    }
    for (const hostKey of hostKeys) {
      await recordEndpointHostKey(supabase, {
        ownerUserId: params.ownerUserId,
        endpointId: params.endpointId,
        algorithm: hostKey.algorithm,
        fingerprintSha256: hostKey.fingerprintSha256,
        generation: params.generation,
      });
      await recordAuditEvent(supabase, {
        ownerUserId: params.ownerUserId,
        instanceId: params.instanceId,
        endpointId: params.endpointId,
        eventType: "host_key_registered",
        detail: {
          algorithm: hostKey.algorithm,
          fingerprint: hostKey.fingerprintSha256,
          generation: params.generation,
        },
        correlationId: params.correlationId,
      });
    }
  } catch (err) {
    await recordAuditEvent(supabase, {
      ownerUserId: params.ownerUserId,
      instanceId: params.instanceId,
      endpointId: params.endpointId,
      eventType: "readiness_stage_failed",
      detail: { stage: "host_key_read", reason: (err as Error).message },
      correlationId: params.correlationId,
    });
  }
}

function requireIdempotencyKey(body: Record<string, unknown>): string {
  const key = body.idempotency_key;
  if (typeof key !== "string" || key.length === 0) {
    throw new ValidationError("idempotency_key is required");
  }
  return key;
}

class ValidationError extends Error {}

async function handleEnsure(
  supabase: SupabaseClient,
  ownerUserId: string,
  // deno-lint-ignore no-explicit-any
  body: Record<string, any>,
  correlationId: string,
  env: ProviderEnv,
): Promise<Response> {
  const elapsed = startTimer();
  let idempotencyKey: string;
  try {
    idempotencyKey = requireIdempotencyKey(body);
  } catch (err) {
    return json({ error: (err as Error).message }, 400, correlationId);
  }

  const region: RegionCode = isRegionCode(body.region)
    ? body.region
    : "us_east";
  const sizePreset: SizePreset = isSizePreset(body.size_preset)
    ? body.size_preset
    : "small";

  const existingOp = await findExistingOperation(
    supabase,
    ownerUserId,
    idempotencyKey,
  );
  if (existingOp) {
    // Repeated request with the same key: never create a second instance.
    const instance = await getInstanceForOwner(supabase, ownerUserId);
    return json(
      {
        operation_id: existingOp.id,
        status: existingOp.status,
        instance: serializeInstance(instance),
      },
      200,
      correlationId,
    );
  }

  const existingInstance = await getInstanceForOwner(supabase, ownerUserId);
  if (existingInstance && existingInstance.status !== "failed") {
    // One managed instance per user (Goal 1): ensure is a no-op once
    // provisioned, regardless of idempotency key, so a second "first open of
    // a managed repo" never provisions a second VM.
    const op = await beginOperation(supabase, {
      ownerUserId,
      instanceId: existingInstance.id,
      operationType: "provision",
      idempotencyKey,
    });
    await completeOperation(supabase, op.id, { status: "succeeded" });
    return json(
      {
        operation_id: op.id,
        status: "succeeded",
        instance: serializeInstance(existingInstance),
      },
      200,
      correlationId,
    );
  }

  const instance = existingInstance?.status === "failed"
    ? existingInstance
    : await createProvisioningInstance(supabase, {
      ownerUserId,
      region,
      sizePreset,
      manifestVersion: CURRENT_MANIFEST_VERSION,
    });
  if (instance.status === "failed") {
    await updateInstance(supabase, instance.id, { status: "provisioning" });
  }

  const op = await beginOperation(supabase, {
    ownerUserId,
    instanceId: instance.id,
    operationType: "provision",
    idempotencyKey,
  });

  await recordAuditEvent(supabase, {
    ownerUserId,
    instanceId: instance.id,
    eventType: "instance_create_requested",
    detail: {
      region,
      size_preset: sizePreset,
      manifest_version: CURRENT_MANIFEST_VERSION,
    },
    correlationId,
    idempotencyKey,
  });

  try {
    const provider = env.provider();
    const providerInstance = await provider.createInstance({
      ownerUserId,
      region,
      sizePreset,
      manifestVersion: CURRENT_MANIFEST_VERSION,
      idempotencyKey,
    });

    await updateInstance(supabase, instance.id, {
      provider_resource_id: providerInstance.providerResourceId,
      status: "bootstrapping",
    });
    const bootstrap = await provider.execOnMachine(
      providerInstance.providerResourceId,
      bootstrapCommand(CURRENT_MANIFEST_VERSION),
      300,
    );
    if (bootstrap.exitCode !== 0) {
      throw new ProviderError(
        "other",
        `Sprite bootstrap exited ${bootstrap.exitCode}: ${bootstrap.stderr || bootstrap.stdout}`,
      );
    }

    const observedInstance = await provider.getInstance(
      providerInstance.providerResourceId,
    );
    const status = mapProviderStateToInstanceStatus(observedInstance.state);
    await updateInstance(supabase, instance.id, {
      provider_resource_id: providerInstance.providerResourceId,
      status,
      ready_at: status === "ready" ? new Date().toISOString() : null,
    });

    // The endpoint row names sshd as the relay sees it from inside the
    // Sprite (loopback). Clients never dial this address: they connect
    // through the relay transport that `issue_certificate` returns.
    const endpointId = await recordManagedEndpoint(supabase, {
      ownerUserId,
      instanceId: instance.id,
      hostname: MANAGED_SSHD_HOST,
      port: MANAGED_SSHD_PORT,
      username: MANAGED_SSH_USERNAME,
      existingEndpointId: null,
    });
    await updateInstance(supabase, instance.id, { endpoint_id: endpointId });
    await establishSshTrust(supabase, provider, {
      ownerUserId,
      instanceId: instance.id,
      endpointId,
      providerResourceId: providerInstance.providerResourceId,
      generation: 0,
      correlationId,
      stub: env.stub,
    });

    await completeOperation(supabase, op.id, {
      status: "succeeded",
      providerRequestId: providerInstance.providerResourceId,
    });
    await recordAuditEvent(supabase, {
      ownerUserId,
      instanceId: instance.id,
      eventType: "instance_create_succeeded",
      detail: {
        provider_resource_id: providerInstance.providerResourceId,
        observed_state: observedInstance.state,
      },
      correlationId,
      providerRequestId: providerInstance.providerResourceId,
      durationMs: elapsed(),
    });

    const refreshed = await getInstanceForOwner(supabase, ownerUserId);
    return json(
      {
        operation_id: op.id,
        status: "succeeded",
        instance: serializeInstance(refreshed),
      },
      200,
      correlationId,
    );
  } catch (err) {
    await updateInstance(supabase, instance.id, { status: "failed" });
    await completeOperation(supabase, op.id, {
      status: "failed",
      errorMessage: (err as Error).message,
    });
    await recordAuditEvent(supabase, {
      ownerUserId,
      instanceId: instance.id,
      eventType: "instance_create_failed",
      detail: { error: (err as Error).message },
      correlationId,
      durationMs: elapsed(),
    });
    throw err;
  }
}

async function handleWake(
  supabase: SupabaseClient,
  ownerUserId: string,
  // deno-lint-ignore no-explicit-any
  body: Record<string, any>,
  correlationId: string,
  env: ProviderEnv,
): Promise<Response> {
  const elapsed = startTimer();
  let idempotencyKey: string;
  try {
    idempotencyKey = requireIdempotencyKey(body);
  } catch (err) {
    return json({ error: (err as Error).message }, 400, correlationId);
  }

  const instance = await requireOwnedInstance(
    supabase,
    ownerUserId,
    body.instance_id,
  );
  if (!instance.provider_resource_id)
    return json(
      { error: "Instance has no provider resource yet" },
      409,
      correlationId,
    );

  const existingOp = await findExistingOperation(
    supabase,
    ownerUserId,
    idempotencyKey,
  );
  if (existingOp)
    return json(
      { operation_id: existingOp.id, status: existingOp.status },
      200,
      correlationId,
    );

  const op = await beginOperation(supabase, {
    ownerUserId,
    instanceId: instance.id,
    operationType: "wake",
    idempotencyKey,
  });
  await recordAuditEvent(supabase, {
    ownerUserId,
    instanceId: instance.id,
    eventType: "instance_wake_requested",
    correlationId,
    idempotencyKey,
  });

  try {
    await updateInstance(supabase, instance.id, { status: "waking" });
    const provider = env.provider();
    await provider.wakeInstance(instance.provider_resource_id);
    const providerInstance = await provider.getInstance(
      instance.provider_resource_id,
    );
    const status = mapProviderStateToInstanceStatus(providerInstance.state);
    await updateInstance(supabase, instance.id, {
      status,
      ready_at:
        status === "ready" ? new Date().toISOString() : instance.ready_at,
    });
    await completeOperation(supabase, op.id, { status: "succeeded" });
    await recordAuditEvent(supabase, {
      ownerUserId,
      instanceId: instance.id,
      eventType: "instance_wake_succeeded",
      detail: { observed_state: providerInstance.state },
      correlationId,
      durationMs: elapsed(),
    });
    return json(
      { operation_id: op.id, status: "succeeded" },
      200,
      correlationId,
    );
  } catch (err) {
    await completeOperation(supabase, op.id, {
      status: "failed",
      errorMessage: (err as Error).message,
    });
    await recordAuditEvent(supabase, {
      ownerUserId,
      instanceId: instance.id,
      eventType: "instance_wake_failed",
      detail: { error: (err as Error).message },
      correlationId,
      durationMs: elapsed(),
    });
    throw err;
  }
}

async function handleReprovision(
  supabase: SupabaseClient,
  ownerUserId: string,
  // deno-lint-ignore no-explicit-any
  body: Record<string, any>,
  correlationId: string,
  env: ProviderEnv,
): Promise<Response> {
  const elapsed = startTimer();
  let idempotencyKey: string;
  try {
    idempotencyKey = requireIdempotencyKey(body);
  } catch (err) {
    return json({ error: (err as Error).message }, 400, correlationId);
  }

  const instance = await requireOwnedInstance(
    supabase,
    ownerUserId,
    body.instance_id,
  );
  if (!instance.provider_resource_id)
    return json(
      { error: "Instance has no provider resource yet" },
      409,
      correlationId,
    );

  const region: RegionCode = isRegionCode(body.region)
    ? body.region
    : instance.region;
  const sizePreset: SizePreset = isSizePreset(body.size_preset)
    ? body.size_preset
    : instance.size_preset;

  const existingOp = await findExistingOperation(
    supabase,
    ownerUserId,
    idempotencyKey,
  );
  if (existingOp)
    return json(
      { operation_id: existingOp.id, status: existingOp.status },
      200,
      correlationId,
    );

  const op = await beginOperation(supabase, {
    ownerUserId,
    instanceId: instance.id,
    operationType: "reprovision",
    idempotencyKey,
  });
  // Repair is in-place for Sprites. The generation identifies the durable
  // provider resource and changes only when that identity changes.
  const nextGeneration = instance.generation;
  await recordAuditEvent(supabase, {
    ownerUserId,
    instanceId: instance.id,
    eventType: "instance_replace_requested",
    detail: {
      region,
      size_preset: sizePreset,
      from_generation: instance.generation,
      to_generation: nextGeneration,
    },
    correlationId,
    idempotencyKey,
  });

  try {
    await updateInstance(supabase, instance.id, { status: "reprovisioning" });
    const provider = env.provider();
    const providerInstance = await provider.replaceInstance({
      providerResourceId: instance.provider_resource_id,
      region,
      sizePreset,
      manifestVersion: CURRENT_MANIFEST_VERSION,
      idempotencyKey,
    });
    const bootstrap = await provider.execOnMachine(
      providerInstance.providerResourceId,
      bootstrapCommand(CURRENT_MANIFEST_VERSION),
      300,
    );
    if (bootstrap.exitCode !== 0) {
      throw new ProviderError(
        "other",
        `Sprite repair exited ${bootstrap.exitCode}: ${bootstrap.stderr || bootstrap.stdout}`,
      );
    }

    const repairedInstance = await provider.getInstance(
      providerInstance.providerResourceId,
    );
    const status = mapProviderStateToInstanceStatus(repairedInstance.state);
    await updateInstance(supabase, instance.id, {
      status,
      generation: nextGeneration,
      size_preset: sizePreset,
      image_manifest_version: CURRENT_MANIFEST_VERSION,
      ready_at: status === "ready" ? new Date().toISOString() : null,
    });

    const endpointId = await recordManagedEndpoint(supabase, {
      ownerUserId,
      instanceId: instance.id,
      hostname: MANAGED_SSHD_HOST,
      port: MANAGED_SSHD_PORT,
      username: MANAGED_SSH_USERNAME,
      existingEndpointId: instance.endpoint_id,
    });
    if (!instance.endpoint_id)
      await updateInstance(supabase, instance.id, {
        endpoint_id: endpointId,
      });

    const previousFingerprint = instance.endpoint_id
      ? await previousHostKeyFingerprint(supabase, instance.endpoint_id)
      : null;

    // Real host key read from the (possibly replaced) VM, recorded at the new
    // generation, plus CA trust re-install (a replacement VM starts from
    // the base image and does not inherit the previous machine's sshd
    // config). This is the explicit host-key rotation record the PRD's
    // "Reprovisioning may rotate the host key" paragraph calls for: old
    // fingerprint, new fingerprint, generation, timestamp, and provider
    // resource id are all captured here (initiating principal is
    // `ownerUserId`, the only principal that can call reprovision).
    await establishSshTrust(supabase, provider, {
      ownerUserId,
      instanceId: instance.id,
      endpointId,
      providerResourceId: providerInstance.providerResourceId,
      generation: nextGeneration,
      correlationId,
      stub: env.stub,
    });
    await recordAuditEvent(supabase, {
      ownerUserId,
      instanceId: instance.id,
      endpointId,
      eventType: "host_key_rotated",
      detail: {
        previous_fingerprint: previousFingerprint,
        generation: nextGeneration,
        provider_resource_id: providerInstance.providerResourceId,
        initiating_principal: ownerUserId,
      },
      correlationId,
    });

    await completeOperation(supabase, op.id, {
      status: "succeeded",
      providerRequestId: providerInstance.providerResourceId,
    });
    await recordAuditEvent(supabase, {
      ownerUserId,
      instanceId: instance.id,
      eventType: "instance_replace_succeeded",
      detail: {
        generation: nextGeneration,
        observed_state: repairedInstance.state,
      },
      correlationId,
      providerRequestId: providerInstance.providerResourceId,
      durationMs: elapsed(),
    });

    const refreshed = await getInstanceForOwner(supabase, ownerUserId);
    return json(
      {
        operation_id: op.id,
        status: "succeeded",
        instance: serializeInstance(refreshed),
      },
      200,
      correlationId,
    );
  } catch (err) {
    await completeOperation(supabase, op.id, {
      status: "failed",
      errorMessage: (err as Error).message,
    });
    await recordAuditEvent(supabase, {
      ownerUserId,
      instanceId: instance.id,
      eventType: "instance_replace_failed",
      detail: { error: (err as Error).message },
      correlationId,
      durationMs: elapsed(),
    });
    throw err;
  }
}

async function handleDelete(
  supabase: SupabaseClient,
  ownerUserId: string,
  // deno-lint-ignore no-explicit-any
  body: Record<string, any>,
  correlationId: string,
  env: ProviderEnv,
): Promise<Response> {
  const elapsed = startTimer();
  let idempotencyKey: string;
  try {
    idempotencyKey = requireIdempotencyKey(body);
  } catch (err) {
    return json({ error: (err as Error).message }, 400, correlationId);
  }

  // Replay before the ownership lookup: once the delete succeeded the owner
  // has no live instance, and a retry must still return the stored result.
  const existingOp = await findExistingOperation(
    supabase,
    ownerUserId,
    idempotencyKey,
  );
  if (existingOp)
    return json(
      { operation_id: existingOp.id, status: existingOp.status },
      200,
      correlationId,
    );

  const instance = await requireOwnedInstance(
    supabase,
    ownerUserId,
    body.instance_id,
  );

  const op = await beginOperation(supabase, {
    ownerUserId,
    instanceId: instance.id,
    operationType: "delete",
    idempotencyKey,
  });
  await recordAuditEvent(supabase, {
    ownerUserId,
    instanceId: instance.id,
    eventType: "instance_delete_requested",
    correlationId,
    idempotencyKey,
  });

  try {
    await updateInstance(supabase, instance.id, { status: "deleting" });
    if (instance.provider_resource_id) {
      const provider = env.provider();
      await provider.deleteInstance(instance.provider_resource_id);
    }
    await updateInstance(supabase, instance.id, {
      status: "deleted",
      endpoint_id: null,
    });
    await completeOperation(supabase, op.id, { status: "succeeded" });
    await recordAuditEvent(supabase, {
      ownerUserId,
      instanceId: instance.id,
      eventType: "instance_delete_succeeded",
      correlationId,
      durationMs: elapsed(),
    });
    return json(
      { operation_id: op.id, status: "succeeded" },
      200,
      correlationId,
    );
  } catch (err) {
    await completeOperation(supabase, op.id, {
      status: "failed",
      errorMessage: (err as Error).message,
    });
    await recordAuditEvent(supabase, {
      ownerUserId,
      instanceId: instance.id,
      eventType: "instance_delete_failed",
      detail: { error: (err as Error).message },
      correlationId,
      durationMs: elapsed(),
    });
    throw err;
  }
}

// Resolves the instance to act on and verifies ownership server-side, per
// the PRD's security requirement to check both principal and resource
// ownership rather than trusting a client-supplied instance id alone. A
// caller may omit instance_id (there is only ever one managed instance per
// user); if they supply one, it must match the caller's own instance.
async function requireOwnedInstance(
  supabase: SupabaseClient,
  ownerUserId: string,
  suppliedInstanceId: unknown,
): Promise<InstanceRow> {
  const instance = await getInstanceForOwner(supabase, ownerUserId);
  if (!instance)
    throw new ValidationErrorWithStatus(
      "No managed instance for this user",
      404,
    );
  if (
    typeof suppliedInstanceId === "string" &&
    suppliedInstanceId !== instance.id
  ) {
    throw new ValidationErrorWithStatus(
      "Instance does not belong to this user",
      403,
    );
  }
  return instance;
}

class ValidationErrorWithStatus extends Error {
  constructor(
    message: string,
    public readonly status: number,
  ) {
    super(message);
  }
}

// PRD "Resource quotas": "purchasing additional disk or compute as a plan
// add-on is explicitly deferred ... and must not be implemented yet, only
// the enforcement of the base limits." A `size_preset` above the base
// allocation must fail as a distinct, structured error - never silently
// downgraded to the base allocation and never surfaced as a generic
// provider/validation failure - so the UI can explain that add-ons aren't
// available yet rather than guessing why provisioning was rejected.
function mapProviderStateToInstanceStatus(state: string): string {
  // Provider states map 1:1 onto the domain lifecycle states already
  // enumerated in the remote_instances status check constraint.
  return state;
}
