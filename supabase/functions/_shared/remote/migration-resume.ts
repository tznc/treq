// Resuming a parked migration (docs/design/sandbox-provider-adapter.md §7).
// A migration whose preflight found unpushed work waits in `awaiting_ack`.
// The owner (`acknowledge_migration`) or an operator (`force_migration`)
// moves it on. Preflight runs again first: if the work has been pushed
// since, the migration goes ahead without recording an acknowledgement.
// A parked drain past its deadline is never forced automatically; the
// hourly run reports it as overdue.
//
// Like the engine, this holds no Supabase code, so tests run it on fakes.

import { type EngineDeps, type MigrationRow, runPreflightOnSource, stopSourceAgain } from "./migration-engine.ts";
import type { PreflightReport } from "./preflight.ts";

export interface ResumeResult {
  /** True when the migration left `awaiting_ack`. */
  resumed: boolean;
  state: string;
  /** True when the user's unpushed work is now acknowledged as lost. */
  acknowledged: boolean;
  /** The fresh preflight, or null when the migration was not parked. */
  preflight: PreflightReport | null;
}

export async function resumeParkedMigration(
  deps: Pick<EngineDeps, "store" | "providers" | "setup">,
  migrationId: string,
  opts: { acknowledge: boolean },
): Promise<ResumeResult> {
  const { store } = deps;
  const m = await store.getMigration(migrationId);
  if (!m) throw new Error(`migration ${migrationId} not found`);
  if (m.state !== "awaiting_ack") {
    return { resumed: false, state: m.state, acknowledged: false, preflight: null };
  }
  const { report: preflight, woke } = await runPreflightOnSource(deps, m);
  const acknowledged = preflight.requires_ack && opts.acknowledge;
  await store.setMigrationFields(
    m.id,
    acknowledged ? { preflight, acknowledged_data_loss: true } : { preflight },
  );
  if (preflight.requires_ack && !acknowledged) {
    // Still parked: a source that preflight woke stops again.
    if (woke) await stopSourceAgain(deps, m);
    return { resumed: false, state: m.state, acknowledged: false, preflight };
  }
  // Back to `preflight`, whose step reads the report stored above.
  const ok = await store.advance(m.id, "awaiting_ack", "preflight", { resumed: true, acknowledged });
  const state = ok ? "preflight" : (await store.getMigration(m.id))?.state ?? m.state;
  return { resumed: ok, state, acknowledged, preflight };
}

type SummaryFields = Pick<MigrationRow, "id" | "state" | "reason" | "preflight"> & { deadline?: string | null };

/** What `status` and `resize` tell the app about an open migration. */
export function migrationSummary(m: SummaryFields | null): {
  id: string;
  state: string;
  reason: string;
  deadline: string | null;
  preflight: PreflightReport | null;
} | null {
  if (!m) return null;
  return { id: m.id, state: m.state, reason: m.reason, deadline: m.deadline ?? null, preflight: m.preflight };
}

/** A parked drain or retire past its deadline: ops must decide, nothing forces it. */
export function isOverdueParked(
  m: { state: string; reason: string; deadline?: string | null },
  nowMs: number,
): boolean {
  if (m.state !== "awaiting_ack" || (m.reason !== "drain" && m.reason !== "retire")) return false;
  if (!m.deadline) return false;
  return Date.parse(m.deadline) < nowMs;
}
