// Row and provider-result builders shared by the remote unit tests. Each
// fills the fields a test does not care about with one fixed world: user
// u1's instance "inst" on Daytona in us_east.

import type { MigrationRow, PlacementRow } from "../_shared/remote/migration-engine.ts";
import type { ManagedInstanceState, ProviderInstance } from "../_shared/remote/provider.ts";

export function placementRow(id: string, fields: Partial<PlacementRow> = {}): PlacementRow {
  return {
    id,
    instance_id: "inst",
    owner_user_id: "u1",
    provider_kind: "daytona",
    machine_type_id: "daytona:s",
    region: "us_east",
    provider_resource_id: `m-${id}`,
    state: "suspended",
    role: "retiring",
    ...fields,
  };
}

export function migrationRow(fields: Partial<MigrationRow> = {}): MigrationRow {
  return {
    id: "mig",
    instance_id: "inst",
    owner_user_id: "u1",
    source_placement_id: "src",
    target_placement_id: "tgt",
    target_machine_type_id: "daytona:m",
    reason: "user_resize",
    state: "completed",
    preflight: null,
    acknowledged_data_loss: false,
    attempt: 0,
    ...fields,
  };
}

/** What a provider's getInstance/createInstance answers for machine `id`. */
export function providerInstance(
  id: string,
  state: ManagedInstanceState,
  extra: Partial<ProviderInstance> = {},
): ProviderInstance {
  return { providerResourceId: id, state, region: "us_east", sizePreset: "small", address: null, ...extra };
}
