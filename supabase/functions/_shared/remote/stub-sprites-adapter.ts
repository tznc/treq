// In-memory Sprites adapter for local service-qa, activated when
// TREQ_REMOTE_SPRITES_STUB=1 so the control plane can be exercised end to end
// without a real Fly account or vendor token, mirroring
// `stub-github-adapter.ts`'s role for the merge queue.

import type {
  CreateInstanceParams,
  MachineExecResult,
  ManagedComputeProvider,
  ManagedInstanceState,
  ProviderInstance,
  ReplaceInstanceParams,
  ServiceSpec,
} from "./sprites-adapter.ts";
import { ProviderError } from "./sprites-adapter.ts";

export function isSpritesStubEnabled(): boolean {
  const v = Deno.env.get("TREQ_REMOTE_SPRITES_STUB") ?? "";
  return v === "1" || v.toLowerCase() === "true";
}

interface StubMachine {
  id: string;
  state: ManagedInstanceState;
  region: ProviderInstance["region"];
  sizePreset: ProviderInstance["sizePreset"];
  address: string;
}

// Module-level so repeated Edge Function invocations within the same Deno
// isolate share state, letting a stub-mode acceptance run exercise wake and
// reprovision against a "warm" instance. Each isolate restart resets it,
// which is fine for local/service-qa use.
const machines = new Map<string, StubMachine>();

export class StubSpritesProvider implements ManagedComputeProvider {
  createInstance(params: CreateInstanceParams): Promise<ProviderInstance> {
    const id = `stub-${params.ownerUserId}`;
    const existing = machines.get(id);
    if (existing) {
      return Promise.resolve(toProviderInstance(existing));
    }
    const machine: StubMachine = {
      id,
      state: "ready",
      region: params.region,
      sizePreset: params.sizePreset,
      address: `${id}.stub.internal`,
    };
    machines.set(id, machine);
    return Promise.resolve(toProviderInstance(machine));
  }

  getInstance(providerId: string): Promise<ProviderInstance> {
    const machine = machines.get(providerId);
    if (!machine) throw new ProviderError("not_found", "stub instance not found");
    return Promise.resolve(toProviderInstance(machine));
  }

  wakeInstance(providerId: string): Promise<void> {
    const machine = machines.get(providerId);
    if (!machine) throw new ProviderError("not_found", "stub instance not found");
    machine.state = "ready";
    return Promise.resolve();
  }

  stopInstance(providerId: string): Promise<void> {
    const machine = machines.get(providerId);
    if (machine) machine.state = "suspended";
    return Promise.resolve();
  }

  replaceInstance(params: ReplaceInstanceParams): Promise<ProviderInstance> {
    const machine = machines.get(params.providerResourceId);
    if (!machine) throw new ProviderError("not_found", "stub instance not found");
    machine.region = params.region;
    machine.sizePreset = params.sizePreset;
    machine.state = "ready";
    return Promise.resolve(toProviderInstance(machine));
  }

  deleteInstance(providerId: string): Promise<void> {
    machines.delete(providerId);
    return Promise.resolve();
  }

  execOnMachine(providerId: string, _command: string[], _timeoutSeconds?: number): Promise<MachineExecResult> {
    if (!machines.has(providerId)) throw new ProviderError("not_found", "stub instance not found");
    // Local/service-qa stub: there is no real machine to run a command on, so
    // this records success without touching anything, matching the rest of
    // this adapter's "warm in-memory fake" behavior.
    return Promise.resolve({ exitCode: 0, stdout: "", stderr: "" });
  }

  ensureService(providerId: string, _name: string, _spec: ServiceSpec): Promise<void> {
    if (!machines.has(providerId)) throw new ProviderError("not_found", "stub instance not found");
    return Promise.resolve();
  }
}

function toProviderInstance(machine: StubMachine): ProviderInstance {
  return {
    providerResourceId: machine.id,
    state: machine.state,
    region: machine.region,
    sizePreset: machine.sizePreset,
    address: machine.address,
  };
}
