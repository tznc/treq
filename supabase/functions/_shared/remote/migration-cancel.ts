// The remote-instance `cancel_migration` request: the owner declines a
// migration parked in `awaiting_ack` (a drain, retire or other switch that
// found unpushed work), so the workspace stays where it is. Only a parked
// migration can be declined: nothing has been provisioned for it yet, and the
// source machine keeps running. The cancel is a compare-and-swap from
// `awaiting_ack`, so it never races a worker that resumed the migration, and
// re-sending it reports the migration as cancelled. Open to Free users:
// declining must work even after a subscription lapses.

import type { SupabaseClient } from "npm:@supabase/supabase-js@2.95.3";
import { recordAuditEvent, type RemoteAuditEventType } from "./audit.ts";
import { must } from "./db.ts";
import { advanceMigration } from "./placement-store.ts";

interface CancelMigrationRow {
  id: string;
  instance_id: string;
  owner_user_id: string;
  state: string;
  reason: string;
}

export interface CancelDeps {
  ownerUserId: string;
  correlationId: string;
  getMigration(id: string): Promise<CancelMigrationRow | null>;
  /** `advance_migration`: false when the row was not in `expected`. */
  advance(id: string, expected: string, next: string, detail?: Record<string, unknown>): Promise<boolean>;
  audit(e: { instanceId: string; eventType: RemoteAuditEventType; detail: Record<string, unknown>; idempotencyKey: string }): Promise<void>;
}

type Reply = { status: number; body: Record<string, unknown> };

export async function cancelParkedMigration(deps: CancelDeps, body: Record<string, unknown>): Promise<Reply> {
  const migrationId = body.migration_id;
  if (typeof migrationId !== "string" || migrationId.length === 0) {
    return { status: 400, body: { error: "migration_id is required", code: "validation_error" } };
  }
  const key = body.idempotency_key;
  if (typeof key !== "string" || key.length === 0) {
    return { status: 400, body: { error: "idempotency_key is required", code: "validation_error" } };
  }
  const m = await deps.getMigration(migrationId);
  if (!m || m.owner_user_id !== deps.ownerUserId) {
    return { status: 404, body: { error: "Migration not found", code: "not_found" } };
  }
  const cancelled = (): Reply => ({ status: 200, body: { migration_id: m.id, migration_state: "cancelled" } });
  if (m.state === "cancelled") return cancelled();
  const notParked = (state: string): Reply => ({
    status: 409,
    body: { error: "Only a migration waiting for your answer can be declined", code: "migration_not_parked", migration_id: m.id, migration_state: state },
  });
  if (m.state !== "awaiting_ack") return notParked(m.state);
  if (!(await deps.advance(m.id, "awaiting_ack", "cancelled", { reason: "declined_by_owner" }))) {
    const now = await deps.getMigration(m.id);
    return now?.state === "cancelled" ? cancelled() : notParked(now?.state ?? m.state);
  }
  await deps.audit({
    instanceId: m.instance_id,
    eventType: "migration_cancelled",
    detail: { migration_id: m.id, reason: m.reason, declined_by_owner: true },
    idempotencyKey: key,
  });
  return cancelled();
}

/** The deps for the edge function, over the service-role client. */
export function cancelDepsFor(o: { supabase: SupabaseClient; ownerUserId: string; correlationId: string }): CancelDeps {
  return {
    ownerUserId: o.ownerUserId,
    correlationId: o.correlationId,
    getMigration: async (id) =>
      must(
        await o.supabase.from("sandbox_migrations").select("id, instance_id, owner_user_id, state, reason").eq("id", id).maybeSingle(),
        "read migration",
      ) as CancelMigrationRow | null,
    advance: (id, expected, next, detail) => advanceMigration(o.supabase, id, expected, next, detail),
    audit: (e) =>
      recordAuditEvent(o.supabase, {
        ownerUserId: o.ownerUserId,
        instanceId: e.instanceId,
        eventType: e.eventType,
        detail: e.detail,
        correlationId: o.correlationId,
        idempotencyKey: e.idempotencyKey,
      }),
  };
}
