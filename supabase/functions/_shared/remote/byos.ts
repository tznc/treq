// Bring your own sandbox (BYOS; docs/design/sandbox-provider-adapter.md,
// "BYOS"). A Pro user registers their own provider API key and runs their
// cloud workspace on their own provider account. treq bills 5% of the
// provider's list price for that usage (045_provider_credentials.sql).
//
// * providerForPlacement() is the one way to reach a placement's provider:
//   a BYOS placement uses its owner's key, loaded server-side from Vault;
//   it never falls back to treq's account. A 401/403 marks the key invalid.
// * handleByosAction() serves the credential actions of remote-instance.
// * planByosPlacement() picks the machine type for ensure/resize in BYOS
//   mode.
//
// The backend is always on; the desktop's `byos` feature preview is the
// only flag. treq_internal.user_can_byos() (Pro today) is the plan gate.

import type { SupabaseClient } from "npm:@supabase/supabase-js@2.95.3";
import type { MachineTypeRow } from "./migration-engine.ts";
import { providerFor, type ProviderResolver } from "./provider-registry.ts";
import {
  type CreateInstanceParams,
  type DeleteInstanceOptions,
  type GatewayEndpoint,
  type MachineExecResult,
  type ProviderCapabilities,
  type ProviderCredential,
  ProviderError,
  type ProviderInstance,
  type ProviderKind,
  type ReplaceInstanceParams,
  type SandboxProvider,
  type ServiceSpec,
} from "./provider.ts";
import { must } from "./db.ts";

export type CredentialSource = "managed" | "byos";

/** Builds an adapter, with the user's key when given (`providerFor`). */
export type ProviderBuilder = (kind: ProviderKind, credential?: ProviderCredential) => SandboxProvider;

export interface PlacementCredential {
  credential_source: CredentialSource;
  credential_id: string | null;
  /** The decrypted key. Never logged or returned to a client. */
  key: string | null;
}

export interface CredentialMeta {
  id: string;
  provider_kind: string;
  label: string | null;
  validated_at: string | null;
  invalid_at: string | null;
}

export interface ByosCredentialStore {
  loadForPlacement(placementId: string): Promise<PlacementCredential | null>;
  markInvalid(credentialId: string): Promise<void>;
  save(kind: ProviderKind, key: string, label: string | null): Promise<CredentialMeta>;
  list(): Promise<CredentialMeta[]>;
  /** Throws ByosError("credential_in_use") while a live workspace uses it. */
  remove(kind: string): Promise<void>;
  byosCatalog(): Promise<unknown[]>;
}

export class ByosError extends Error {}

/** The code the app sees for a provider error; a bad BYOS key is its own. */
export function providerErrorCode(err: ProviderError): string {
  return err.kind === "unauthorized" ? "provider_credential_invalid" : err.kind;
}

/** Operator drains, retires and cheaper-default moves leave BYOS workspaces alone. */
export function isOperatorMovable(p: { credential_source?: string | null }): boolean {
  return p.credential_source !== "byos";
}

// ── Reaching a placement's provider ───────────────────────────────────

class PlacementProvider implements SandboxProvider {
  private resolved: Promise<{ provider: SandboxProvider; credentialId: string | null }> | null = null;

  constructor(
    private readonly store: Pick<ByosCredentialStore, "loadForPlacement" | "markInvalid">,
    readonly kind: ProviderKind,
    private readonly placementId: string | null,
    private readonly build: ProviderBuilder,
  ) {}

  capabilities(): ProviderCapabilities {
    // Capabilities do not depend on the key; a placeholder avoids needing
    // treq's secret on a BYOS-only path. No request is made.
    return this.build(this.kind, { apiKey: "-" }).capabilities();
  }

  private async load(): Promise<{ provider: SandboxProvider; credentialId: string | null }> {
    if (!this.placementId) return { provider: this.build(this.kind), credentialId: null };
    const row = await this.store.loadForPlacement(this.placementId);
    if (!row || row.credential_source !== "byos") return { provider: this.build(this.kind), credentialId: null };
    if (!row.key) throw new ProviderError("unauthorized", "the placement's BYOS credential is gone");
    return { provider: this.build(this.kind, { apiKey: row.key }), credentialId: row.credential_id };
  }

  private async call<T>(fn: (p: SandboxProvider) => Promise<T>): Promise<T> {
    if (!this.resolved) {
      this.resolved = this.load();
      this.resolved.catch(() => (this.resolved = null));
    }
    const { provider, credentialId } = await this.resolved;
    try {
      return await fn(provider);
    } catch (err) {
      if (err instanceof ProviderError && err.kind === "unauthorized" && credentialId) {
        await this.store.markInvalid(credentialId).catch(() => {});
      }
      throw err;
    }
  }

  createInstance(params: CreateInstanceParams): Promise<ProviderInstance> {
    return this.call((p) => p.createInstance(params));
  }
  getInstance(id: string): Promise<ProviderInstance> {
    return this.call((p) => p.getInstance(id));
  }
  wakeInstance(id: string): Promise<void> {
    return this.call((p) => p.wakeInstance(id));
  }
  stopInstance(id: string): Promise<void> {
    return this.call((p) => p.stopInstance(id));
  }
  replaceInstance(params: ReplaceInstanceParams): Promise<ProviderInstance> {
    return this.call((p) => p.replaceInstance(params));
  }
  deleteInstance(id: string, opts?: DeleteInstanceOptions): Promise<void> {
    return this.call((p) => p.deleteInstance(id, opts));
  }
  execOnMachine(
    id: string,
    command: string[],
    timeoutSeconds?: number,
    environment?: Readonly<Record<string, string>>,
  ): Promise<MachineExecResult> {
    return this.call((p) => p.execOnMachine(id, command, timeoutSeconds, environment));
  }
  ensureService(id: string, name: string, spec: ServiceSpec): Promise<void> {
    return this.call((p) => p.ensureService(id, name, spec));
  }
  keepAlive(id: string): Promise<void> {
    return this.call((p) => p.keepAlive?.(id) ?? Promise.resolve());
  }
  gatewayEndpoint(id: string): Promise<GatewayEndpoint> {
    return this.call((p) => p.gatewayEndpoint(id));
  }
  validateCredentials(): Promise<void> {
    return this.call((p) => p.validateCredentials?.() ?? Promise.resolve());
  }
}

/**
 * The provider for one placement. Without a placement id it is treq's own
 * account. The credential is loaded on first use, once.
 */
export function providerForPlacement(
  store: Pick<ByosCredentialStore, "loadForPlacement" | "markInvalid">,
  kind: ProviderKind,
  placementId: string | null,
  build: ProviderBuilder = (k, c) => providerFor(k, undefined, c),
): SandboxProvider {
  return new PlacementProvider(store, kind, placementId, build);
}

/** Service-role credential loading for workers that act on any owner's placement. */
export function supabaseCredentialLoader(
  supabase: SupabaseClient,
): Pick<ByosCredentialStore, "loadForPlacement" | "markInvalid"> {
  return {
    async loadForPlacement(placementId) {
      const data = must(await supabase.rpc("provider_credential_load_for_placement", {
        p_placement_id: placementId,
      }), "load placement credential");
      return (data as PlacementCredential | null) ?? null;
    },
    async markInvalid(credentialId) {
      must(await supabase.rpc("provider_credential_mark_invalid", { p_credential_id: credentialId }), "mark credential invalid");
    },
  };
}

/** The production resolver every Edge Function injects (service-role client). */
export function placementProviders(supabase: SupabaseClient): ProviderResolver {
  const loader = supabaseCredentialLoader(supabase);
  return (kind, placementId) => providerForPlacement(loader, kind, placementId ?? null);
}

// ── Credential actions (remote-instance) ──────────────────────────────

export const BYOS_ACTIONS: ReadonlySet<string> = new Set([
  "save_provider_credential",
  "list_provider_credentials",
  "delete_provider_credential",
  "byos_catalog",
]);

export interface ByosActionDeps {
  /** can_byos for the caller (the database gate). */
  canByos: () => Promise<boolean>;
  store: ByosCredentialStore;
  build: ProviderBuilder;
}

type ActionResult = { status: number; body: Record<string, unknown> };

const refuse = (status: number, code: string, error: string): ActionResult => ({ status, body: { error, code } });

/** Null when `action` is not a BYOS action. */
export async function handleByosAction(
  action: unknown,
  body: Record<string, unknown>,
  deps: ByosActionDeps,
): Promise<ActionResult | null> {
  if (typeof action !== "string" || !BYOS_ACTIONS.has(action)) return null;
  switch (action) {
    case "list_provider_credentials":
      return { status: 200, body: { credentials: await deps.store.list() } };
    case "byos_catalog":
      return { status: 200, body: { machine_types: await deps.store.byosCatalog() } };
    case "delete_provider_credential": {
      if (typeof body.provider_kind !== "string") return refuse(400, "validation_error", "provider_kind is required");
      try {
        await deps.store.remove(body.provider_kind);
      } catch (err) {
        if (err instanceof ByosError) {
          return refuse(409, err.message, "A cloud workspace still runs on this key. Delete it first.");
        }
        throw err;
      }
      return { status: 200, body: { deleted: body.provider_kind } };
    }
    case "save_provider_credential":
      return await saveCredential(body, deps);
  }
  return null;
}

async function saveCredential(body: Record<string, unknown>, deps: ByosActionDeps): Promise<ActionResult> {
  if (!(await deps.canByos())) {
    return refuse(403, "byos_not_allowed", "Bring your own sandbox needs Pro");
  }
  const kind = body.provider_kind;
  const key = body.api_key;
  const label = body.label;
  if (typeof key !== "string" || key.length === 0 || key.length > 4096) {
    return refuse(400, "validation_error", "api_key is required");
  }
  // Printable ASCII only: a header cannot carry anything else, and fetch's
  // TypeError would otherwise read as a provider outage worth retrying.
  if (!/^[\x20-\x7e]+$/.test(key)) {
    return refuse(400, "validation_error", "api_key has characters an API key cannot contain");
  }
  if (label !== undefined && label !== null && (typeof label !== "string" || label.length > 80)) {
    return refuse(400, "validation_error", "label must be at most 80 characters");
  }
  let provider: SandboxProvider;
  try {
    if (typeof kind !== "string" || kind === "local") throw new ProviderError("invalid_request", "unsupported");
    provider = deps.build(kind as ProviderKind, { apiKey: key });
  } catch {
    return refuse(400, "byos_kind_unsupported", "This provider does not take your own key");
  }
  if (!provider.validateCredentials) {
    return refuse(400, "byos_kind_unsupported", "This provider does not take your own key");
  }
  try {
    await provider.validateCredentials();
  } catch (err) {
    if (err instanceof ProviderError && err.kind === "unauthorized") {
      return refuse(400, "provider_credential_invalid", "The provider rejected this API key");
    }
    if (err instanceof ProviderError) {
      return refuse(502, err.kind, "Could not check the key with the provider; try again");
    }
    throw err;
  }
  const credential = await deps.store.save(kind as ProviderKind, key, (label as string | undefined) ?? null);
  return { status: 200, body: { credential } };
}

/** The store remote-instance uses for its caller: service role plus the user's own client. */
export function supabaseByosStore(
  service: SupabaseClient,
  user: SupabaseClient,
  ownerUserId: string,
): ByosCredentialStore & ByosPlanStore {
  return {
    ...supabaseCredentialLoader(service),
    async save(kind, key, label) {
      return must(await service.rpc("provider_credential_save", {
        p_user_id: ownerUserId,
        p_kind: kind,
        p_key: key,
        p_label: label,
      }), "save credential") as CredentialMeta;
    },
    async list() {
      return (must(await user.rpc("list_provider_credentials"), "list credentials") ?? []) as CredentialMeta[];
    },
    async remove(kind) {
      const res = await user.rpc("delete_provider_credential", { p_kind: kind });
      if (res.error?.message.includes("credential_in_use")) throw new ByosError("credential_in_use");
      must(res, "delete credential");
    },
    async byosCatalog() {
      return (must(await user.rpc("get_byos_catalog"), "read BYOS catalog") ?? []) as unknown[];
    },
    async credentialFor(kind) {
      return must(await service
        .from("remote_provider_credentials")
        .select("id, invalid_at")
        .eq("owner_user_id", ownerUserId)
        .eq("provider_kind", kind)
        .maybeSingle(), "read credential") as { id: string; invalid_at: string | null } | null;
    },
    async machineTypes(kind) {
      return (must(await service
        .from("provider_machine_types")
        .select("id, provider_kind, vendor_type, tier_id, regions, selectable")
        .eq("provider_kind", kind), "read machine types") ?? []) as ByosMachineType[];
    },
  };
}

// ── Placement choice (ensure, resize) ─────────────────────────────────

export type PlacementChoice = { mode: "managed" } | { mode: "byos"; kind: ProviderKind };

export function parsePlacementChoice(
  body: Record<string, unknown>,
): { ok: true; value: PlacementChoice } | { ok: false; error: string } {
  const source = body.credential_source;
  if (source === undefined || source === null || source === "managed") return { ok: true, value: { mode: "managed" } };
  if (source !== "byos") return { ok: false, error: "credential_source must be 'managed' or 'byos'" };
  if (typeof body.provider_kind !== "string" || body.provider_kind.length === 0) {
    return { ok: false, error: "provider_kind is required with credential_source 'byos'" };
  }
  return { ok: true, value: { mode: "byos", kind: body.provider_kind as ProviderKind } };
}

export interface ByosMachineType extends MachineTypeRow {
  regions: string[];
  selectable: boolean;
}

export interface ByosPlanStore {
  credentialFor(kind: ProviderKind): Promise<{ id: string; invalid_at: string | null } | null>;
  machineTypes(kind: ProviderKind): Promise<ByosMachineType[]>;
}

export type ByosPlan =
  | { ok: true; machineType: ByosMachineType; credentialId: string }
  | { ok: false; status: number; code: string };

/**
 * The machine type and credential for a BYOS ensure or resize: the chosen
 * provider's type for the tier, whatever treq's cheapest default is. A
 * type treq hides from its own catalog (a draining provider) still runs on
 * the user's account.
 */
export async function planByosPlacement(
  deps: ByosPlanStore & { canByos: () => Promise<boolean> },
  choice: { mode: "byos"; kind: ProviderKind },
  tierId: string,
  region: string,
): Promise<ByosPlan> {
  if (!(await deps.canByos())) return { ok: false, status: 403, code: "byos_not_allowed" };
  const credential = await deps.credentialFor(choice.kind);
  if (!credential) return { ok: false, status: 400, code: "byos_credential_missing" };
  if (credential.invalid_at) return { ok: false, status: 409, code: "provider_credential_invalid" };
  const type = (await deps.machineTypes(choice.kind))
    .filter((t) => t.tier_id === tierId && t.regions.includes(region))
    .sort((a, b) => Number(b.selectable) - Number(a.selectable) || a.id.localeCompare(b.id))[0];
  if (!type) return { ok: false, status: 400, code: "tier_unavailable" };
  return { ok: true, machineType: type, credentialId: credential.id };
}
