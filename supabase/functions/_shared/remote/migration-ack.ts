// The remote-instance `acknowledge_migration` request
// (docs/design/sandbox-provider-adapter.md §7): the owner accepts losing
// unpushed work so a parked switch can go on. Re-sending it is harmless: a
// migration that is no longer parked is reported as it stands.

import type { SupabaseClient } from "npm:@supabase/supabase-js@2.95.3";
import { recordAuditEvent } from "./audit.ts";
import type { MigrationRow } from "./migration-engine.ts";
import { migrationSummary, resumeParkedMigration } from "./migration-resume.ts";
import { engineDeps, INLINE_BUDGET_MS, runLeasedMigrations } from "./migration-runner.ts";
import type { ProviderKind, SandboxProvider } from "./provider.ts";
import { must } from "./db.ts";

export interface AckOptions {
  supabase: SupabaseClient;
  providers: (kind: ProviderKind, placementId?: string | null) => SandboxProvider;
  stub: boolean;
  correlationId: string;
  ownerUserId: string;
}

export async function acknowledgeMigration(
  o: AckOptions,
  body: Record<string, unknown>,
): Promise<{ status: number; body: Record<string, unknown> }> {
  const migrationId = body.migration_id;
  if (typeof migrationId !== "string" || migrationId.length === 0) {
    return { status: 400, body: { error: "migration_id is required", code: "validation_error" } };
  }
  const key = body.idempotency_key;
  if (typeof key !== "string" || key.length === 0) {
    return { status: 400, body: { error: "idempotency_key is required", code: "validation_error" } };
  }
  const data = must(await o.supabase.from("sandbox_migrations").select("*").eq("id", migrationId).maybeSingle(), "read migration");
  const m = data as MigrationRow | null;
  if (!m || m.owner_user_id !== o.ownerUserId) {
    return { status: 404, body: { error: "Migration not found", code: "not_found" } };
  }
  if (m.state !== "awaiting_ack") {
    return {
      status: 200,
      body: { migration_id: m.id, resumed: false, migration_state: m.state, migration: migrationSummary(m) },
    };
  }

  const resumed = await resumeParkedMigration(engineDeps(o), m.id, { acknowledge: true });
  await recordAuditEvent(o.supabase, {
    ownerUserId: o.ownerUserId,
    instanceId: m.instance_id,
    eventType: "migration_acknowledged",
    detail: { migration_id: m.id, resumed: resumed.resumed, acknowledged_data_loss: resumed.acknowledged },
    correlationId: o.correlationId,
    idempotencyKey: key,
  });
  let state = resumed.state;
  if (resumed.resumed) {
    const run = await runLeasedMigrations({
      ...o,
      leaseOwner: `acknowledge:${o.correlationId}`,
      budgetMs: INLINE_BUDGET_MS,
      limit: 1,
      migrationId: m.id,
    });
    state = run.migrations[0]?.state ?? state;
  }
  return {
    status: state === "completed" ? 200 : 202,
    body: {
      migration_id: m.id,
      resumed: resumed.resumed,
      migration_state: state,
      acknowledged_data_loss: resumed.acknowledged,
      preflight: resumed.preflight,
    },
  };
}
