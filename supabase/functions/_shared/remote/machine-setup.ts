// Bringing a fresh machine up to a usable Treq sandbox: the boot manifest,
// the loopback sshd trusting the Treq SSH CA, and the host key recorded on
// the endpoint. `ensure`, `reprovision` and the migration engine's target
// bootstrap all use these, so a switched sandbox is set up exactly like a
// new one.

import type { SupabaseClient } from "npm:@supabase/supabase-js@2.95.3";
import { recordAuditEvent } from "./audit.ts";
import { bootstrapCommand, CURRENT_MANIFEST_VERSION } from "./boot-manifest.ts";
import { recordEndpointHostKey, updateInstance } from "./instance-store.ts";
import { provisionManagedSshd, readManagedHostKeys } from "./managed-sshd.ts";
import type { MachineSetup, MigrationRow, PlacementRow } from "./migration-engine.ts";
import { type ManagedComputeProvider, ProviderError, type SandboxProvider } from "./provider.ts";
import { caKeyMaterialFromEnv, caPublicKeyLine } from "./ssh-cert.ts";
import { generateGatewaySecret } from "./gateway-token.ts";
import {
  GATEWAY_LISTEN,
  GATEWAY_SECRET_ENV,
  GATEWAY_SERVICE_NAME,
  gatewayServiceSpec,
  installGatewaySecretCommand,
  MANAGED_SSHD_LAUNCHER,
  MANAGED_SSHD_SERVICE_NAME,
} from "./ssh-vm-config.ts";
import { must } from "./db.ts";

/** Where per-placement gateway secrets are kept (Vault, via 045_gateway_secrets.sql). */
export interface GatewaySecretStore {
  load(placementId: string): Promise<string | null>;
  save(placementId: string, secret: string): Promise<void>;
}

export function supabaseGatewaySecrets(supabase: SupabaseClient): GatewaySecretStore {
  return {
    async load(placementId) {
      const data = must(await supabase.rpc("gateway_secret_load", { p_placement_id: placementId }), "load gateway secret");
      return (data as string | null) ?? null;
    },
    async save(placementId, secret) {
      must(await supabase.rpc("gateway_secret_save", { p_placement_id: placementId, p_secret: secret }), "save gateway secret");
    },
  };
}

async function execOk(
  provider: ManagedComputeProvider,
  id: string,
  cmd: string[],
  stage: string,
  timeout?: number,
  env?: Record<string, string>,
) {
  const out = await provider.execOnMachine(id, cmd, timeout, env);
  if (out.exitCode !== 0) {
    throw new ProviderError("other", `${stage} exited ${out.exitCode}: ${out.stderr || out.stdout}`);
  }
}

/**
 * Starts the gateway (`treq gateway`, from the `treq` binary the boot
 * manifest installs): makes a new secret for the placement, saves it to
 * Vault before writing it to the machine, then (re)starts the service on
 * `listen`. The secret goes over the exec environment, not the command.
 */
export async function provisionGateway(
  secrets: GatewaySecretStore,
  provider: SandboxProvider,
  providerResourceId: string,
  placementId: string,
  listen = GATEWAY_LISTEN,
): Promise<void> {
  const secret = generateGatewaySecret();
  await secrets.save(placementId, secret);
  await execOk(provider, providerResourceId, installGatewaySecretCommand(), "gateway secret install", undefined, {
    [GATEWAY_SECRET_ENV]: secret,
  });
  await provider.ensureService(providerResourceId, GATEWAY_SERVICE_NAME, gatewayServiceSpec(placementId, provider.gatewayListenAddress ?? listen));
}

/**
 * A pidfile-supervised service (Boat, Daytona) does not survive a stop and
 * start, so after a wake sshd and the gateway are started again. A provider
 * with a real supervisor restarts them on its own.
 */
export async function reensureServicesAfterWake(
  provider: SandboxProvider,
  providerResourceId: string,
  placementId: string | null,
): Promise<void> {
  const caps = provider.capabilities();
  if (caps.serviceSupervisor) return;
  await provider.ensureService(providerResourceId, MANAGED_SSHD_SERVICE_NAME, { cmd: MANAGED_SSHD_LAUNCHER, args: [] });
  if (placementId) {
    await provider.ensureService(providerResourceId, GATEWAY_SERVICE_NAME, gatewayServiceSpec(placementId, provider.gatewayListenAddress));
  }
}

/** Runs the boot manifest. Throws a ProviderError when it fails. */
export async function runBootManifest(
  provider: ManagedComputeProvider,
  providerResourceId: string,
  label = "bootstrap",
): Promise<void> {
  const out = await provider.execOnMachine(providerResourceId, bootstrapCommand(CURRENT_MANIFEST_VERSION), 300);
  if (out.exitCode !== 0) {
    throw new ProviderError("other", `machine ${label} exited ${out.exitCode}: ${out.stderr || out.stdout}`);
  }
}

interface TrustParams {
  ownerUserId: string;
  instanceId: string;
  endpointId: string;
  providerResourceId: string;
  generation: number;
  correlationId: string;
  stub: boolean;
  /** The placement behind this machine; needed to set up treq-gateway. */
  placementId?: string | null;
}

async function recordStubHostKey(supabase: SupabaseClient, p: TrustParams): Promise<void> {
  // The stub adapter has no real machine: record a clearly labeled
  // placeholder so code that expects a host key row still has one.
  await recordEndpointHostKey(supabase, {
    ownerUserId: p.ownerUserId,
    endpointId: p.endpointId,
    algorithm: "ssh-ed25519",
    fingerprintSha256: "SHA256:stub-mode-no-real-host-key",
    generation: p.generation,
  });
  await recordAuditEvent(supabase, {
    ownerUserId: p.ownerUserId,
    instanceId: p.instanceId,
    endpointId: p.endpointId,
    eventType: "host_key_registered",
    detail: {
      note: "TREQ_REMOTE_PROVIDER_STUB active: recorded a placeholder fingerprint, not a real host key",
      generation: p.generation,
    },
    correlationId: p.correlationId,
  });
}

/** Reads the machine's sshd host key and records it at `generation`. */
export async function recordHostKeys(
  supabase: SupabaseClient,
  provider: ManagedComputeProvider,
  p: TrustParams,
): Promise<void> {
  if (p.stub) return await recordStubHostKey(supabase, p);
  try {
    const hostKeys = await readManagedHostKeys(provider, p.providerResourceId);
    if (hostKeys.length === 0) throw new Error("sshd host key file held no ed25519 key");
    for (const hostKey of hostKeys) {
      await recordEndpointHostKey(supabase, {
        ownerUserId: p.ownerUserId,
        endpointId: p.endpointId,
        algorithm: hostKey.algorithm,
        fingerprintSha256: hostKey.fingerprintSha256,
        generation: p.generation,
      });
      await recordAuditEvent(supabase, {
        ownerUserId: p.ownerUserId,
        instanceId: p.instanceId,
        endpointId: p.endpointId,
        eventType: "host_key_registered",
        detail: { algorithm: hostKey.algorithm, fingerprint: hostKey.fingerprintSha256, generation: p.generation },
        correlationId: p.correlationId,
      });
    }
  } catch (err) {
    await recordAuditEvent(supabase, {
      ownerUserId: p.ownerUserId,
      instanceId: p.instanceId,
      endpointId: p.endpointId,
      eventType: "readiness_stage_failed",
      detail: { stage: "host_key_read", reason: (err as Error).message },
      correlationId: p.correlationId,
    });
  }
}

// Brings up the loopback sshd, trusts the Treq SSH CA, and records the real
// host key read over the provider's exec API. A failure is audited as a
// readiness-stage failure rather than failing the whole operation, so a
// later repair or explicit `keyscan_endpoint` can complete it.
export async function establishSshTrust(
  supabase: SupabaseClient,
  provider: ManagedComputeProvider,
  p: TrustParams,
): Promise<void> {
  if (p.stub) return await recordStubHostKey(supabase, p);
  try {
    await provisionManagedSshd(provider, p.providerResourceId, caPublicKeyLine(caKeyMaterialFromEnv()));
    await recordAuditEvent(supabase, {
      ownerUserId: p.ownerUserId,
      instanceId: p.instanceId,
      endpointId: p.endpointId,
      eventType: "ca_trust_installed",
      detail: { generation: p.generation },
      correlationId: p.correlationId,
    });
  } catch (err) {
    await recordAuditEvent(supabase, {
      ownerUserId: p.ownerUserId,
      instanceId: p.instanceId,
      endpointId: p.endpointId,
      eventType: "ca_trust_install_failed",
      detail: { error: (err as Error).message },
      correlationId: p.correlationId,
    });
    return;
  }
  if (p.placementId && "capabilities" in provider) {
    try {
      await provisionGateway(
        supabaseGatewaySecrets(supabase),
        provider as SandboxProvider,
        p.providerResourceId,
        p.placementId,
      );
    } catch (err) {
      await recordAuditEvent(supabase, {
        ownerUserId: p.ownerUserId,
        instanceId: p.instanceId,
        endpointId: p.endpointId,
        eventType: "readiness_stage_failed",
        detail: { stage: "gateway", reason: (err as Error).message },
        correlationId: p.correlationId,
      });
    }
  }
  await recordHostKeys(supabase, provider, p);
}

/**
 * The engine's machine setup against Supabase. The target is bootstrapped
 * like a new sandbox (a failure here rolls the migration back). After
 * cutover its host key is recorded at the instance's new fence, which is
 * the generation clients see in the host-key rotation record.
 */
export function supabaseMachineSetup(
  supabase: SupabaseClient,
  opts: { stub: boolean; correlationId: string; providers: (kind: PlacementRow["provider_kind"], placementId?: string | null) => ManagedComputeProvider },
): MachineSetup {
  return {
    async bootstrap(provider, providerResourceId, placementId) {
      if (opts.stub) return;
      await runBootManifest(provider, providerResourceId);
      await provisionManagedSshd(provider, providerResourceId, caPublicKeyLine(caKeyMaterialFromEnv()));
      if (placementId) {
        await provisionGateway(
          supabaseGatewaySecrets(supabase),
          provider,
          providerResourceId,
          placementId,
        );
      }
    },
    afterWake: (provider, providerResourceId, placementId) =>
      reensureServicesAfterWake(provider, providerResourceId, placementId),
    async afterCutover(migration: MigrationRow, target: PlacementRow) {
      const data = must(await supabase
        .from("remote_instances")
        .select("id, endpoint_id, fence, generation")
        .eq("id", migration.instance_id)
        .maybeSingle(), "read instance");
      if (!data) return;
      const generation = Math.max(Number(data.generation), Number(data.fence));
      if (generation !== Number(data.generation)) await updateInstance(supabase, data.id, { generation });
      if (!data.endpoint_id || !target.provider_resource_id) return;
      await recordHostKeys(supabase, opts.providers(target.provider_kind, target.id), {
        ownerUserId: migration.owner_user_id,
        instanceId: data.id,
        endpointId: data.endpoint_id,
        providerResourceId: target.provider_resource_id,
        generation,
        correlationId: opts.correlationId,
        stub: opts.stub,
      });
      await recordAuditEvent(supabase, {
        ownerUserId: migration.owner_user_id,
        instanceId: data.id,
        endpointId: data.endpoint_id,
        eventType: "host_key_rotated",
        detail: {
          generation,
          provider_resource_id: target.provider_resource_id,
          migration_id: migration.id,
          initiating_principal: "migration",
        },
        correlationId: opts.correlationId,
      });
    },
  };
}
