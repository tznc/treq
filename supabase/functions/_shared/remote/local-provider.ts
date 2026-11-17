// In-memory provider for local testing of multi-provider flows. Under
// TREQ_REMOTE_PROVIDER_STUB=1, `providerFor` returns one for every vendor
// kind, with that vendor's capabilities, so local service-qa never calls a
// real vendor. Commands run nowhere: `setLocalExecHandler` scripts their output.

import type {
  CreateInstanceParams,
  MachineExecResult,
  ManagedInstanceState,
  ProviderCapabilities,
  ProviderInstance,
  ProviderKind,
  ReplaceInstanceParams,
  SandboxProvider,
  ServiceSpec,
  GatewayEndpoint,
} from "./provider.ts";
import { ProviderError } from "./provider.ts";

const LOCAL_CAPABILITIES: Readonly<ProviderCapabilities> = Object.freeze({
  stopKeepsDisk: true,
  execApi: true,
  serviceSupervisor: true,
  tcpProxy: false,
  billingGranularitySec: 1,
});

export type LocalExecHandler = (
  providerId: string,
  command: string[],
  environment: Readonly<Record<string, string>>,
) => MachineExecResult;

interface LocalMachine {
  id: string;
  state: ManagedInstanceState;
  region: ProviderInstance["region"];
  sizePreset: ProviderInstance["sizePreset"];
  services: Map<string, ServiceSpec>;
}

// Module-level so repeated Edge Function invocations within the same Deno
// isolate share machines, letting a stub-mode run exercise wake and
// reprovision. Each isolate restart resets it.
const machines = new Map<string, LocalMachine>();
/** True when TREQ_REMOTE_PROVIDER_STUB=1: every provider is a LocalProvider. */
export function isProviderStubEnabled(
  env: (key: string) => string | undefined = (key) => Deno.env.get(key),
): boolean {
  const v = env("TREQ_REMOTE_PROVIDER_STUB") ?? "";
  return v === "1" || v.toLowerCase() === "true";
}

let execHandler: LocalExecHandler | null = null;

/** Test hook: scripts every exec's result. `null` restores exit 0, no output. */
export function setLocalExecHandler(handler: LocalExecHandler | null): void {
  execHandler = handler;
}

export class LocalProvider implements SandboxProvider {
  constructor(
    readonly kind: ProviderKind = "local",
    private readonly caps: Readonly<ProviderCapabilities> = LOCAL_CAPABILITIES,
  ) {}

  capabilities(): ProviderCapabilities {
    return { ...this.caps };
  }

  private machine(providerId: string): LocalMachine {
    const machine = machines.get(providerId);
    if (!machine) throw new ProviderError("not_found", "local instance not found");
    return machine;
  }

  createInstance(params: CreateInstanceParams): Promise<ProviderInstance> {
    // One machine per owner and kind, so a repeated create returns it.
    const id = `local-${this.kind}-${params.ownerUserId}`;
    let machine = machines.get(id);
    if (!machine) {
      machine = {
        id,
        state: "ready",
        region: params.region,
        sizePreset: params.sizePreset,
        services: new Map(),
      };
      machines.set(id, machine);
    }
    return Promise.resolve(toProviderInstance(machine));
  }

  // Accepts any key, so service-qa can exercise BYOS without a vendor.
  validateCredentials(): Promise<void> {
    return Promise.resolve();
  }

  getInstance(providerId: string): Promise<ProviderInstance> {
    return Promise.resolve().then(() => toProviderInstance(this.machine(providerId)));
  }

  wakeInstance(providerId: string): Promise<void> {
    return Promise.resolve().then(() => {
      this.machine(providerId).state = "ready";
    });
  }

  stopInstance(providerId: string): Promise<void> {
    const machine = machines.get(providerId);
    if (machine) machine.state = "suspended";
    return Promise.resolve();
  }

  replaceInstance(params: ReplaceInstanceParams): Promise<ProviderInstance> {
    return Promise.resolve().then(() => {
      const machine = this.machine(params.providerResourceId);
      machine.region = params.region;
      machine.sizePreset = params.sizePreset;
      machine.state = "ready";
      return toProviderInstance(machine);
    });
  }

  deleteInstance(providerId: string): Promise<void> {
    machines.delete(providerId);
    return Promise.resolve();
  }

  execOnMachine(
    providerId: string,
    command: string[],
    _timeoutSeconds?: number,
    environment: Readonly<Record<string, string>> = {},
  ): Promise<MachineExecResult> {
    return Promise.resolve().then(() => {
      this.machine(providerId);
      return execHandler?.(providerId, command, environment) ??
        { exitCode: 0, stdout: "", stderr: "" };
    });
  }

  ensureService(providerId: string, name: string, spec: ServiceSpec): Promise<void> {
    return Promise.resolve().then(() => {
      this.machine(providerId).services.set(name, spec);
    });
  }

  gatewayEndpoint(_providerId: string): Promise<GatewayEndpoint> {
    return Promise.reject(new ProviderError("unavailable", "the local provider has no TCP path"));
  }
}

function toProviderInstance({ id, state, region, sizePreset }: LocalMachine): ProviderInstance {
  return { providerResourceId: id, state, region, sizePreset, address: null };
}
