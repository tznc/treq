// Monthly spend cap for Pro cloud workspaces
// (docs/design/sandbox-provider-adapter.md §5, 048_spend_cap.sql).
//
// remote-admin's `enforce_spend_caps` action runs enforceSpendCaps() hourly
// after `report_usage` and every 15 minutes. For each user at 80% of their
// cap or more this billing period (the Stripe period, else the UTC month):
//
// * 80%: one "spend_cap_warning" notice per month.
// * 100%: one "spend_cap_reached" notice per month, and every running
//   workspace is stopped through its provider and set_placement_state, as
//   the lapse sweep does. Stops repeat on every run while the user is capped.
//
// remote-instance refuses `ensure`, `wake` and `resize` for a capped user
// with spendCapRefusal(): 402 {code: "spend_cap_reached", cap_usd, spent_usd}.

import type { SupabaseClient } from "npm:@supabase/supabase-js@2.95.3";
import { setPlacementState, stopWorkspace } from "../remote/placement-store.ts";
import type { ManagedComputeProvider, ProviderKind } from "../remote/provider.ts";
import { must, rpc } from "../remote/db.ts";

export type SpendState = "ok" | "warning" | "capped";

/** get_spend_status() / get_spend_status_for(). */
export interface SpendStatus {
  cap_usd: number;
  spent_usd: number;
  month_start: string;
  state: SpendState;
}

/** A row of spend_cap_candidates(). */
export interface SpendCapCandidate extends SpendStatus {
  user_id: string;
  state: "warning" | "capped";
}

export interface RunningWorkspace {
  instance_id: string;
  provider_kind: ProviderKind;
  provider_resource_id: string | null;
  active_placement_id: string | null;
}

export interface SpendNotice {
  userId: string;
  kind: "spend_cap_warning" | "spend_cap_reached";
  /** The once-per-period key: YYYY-MM for a calendar month, YYYY-MM-DDTHH for a Stripe period. */
  period: string;
  title: string;
  body: string;
}

export interface SpendCapStore {
  candidates(now: Date): Promise<SpendCapCandidate[]>;
  /** True when stored; false when already sent this month. */
  notify(notice: SpendNotice): Promise<boolean>;
  runningWorkspaces(userId: string): Promise<RunningWorkspace[]>;
  markStopped(instanceId: string, activePlacementId: string | null): Promise<void>;
  /**
   * Sends the user's open `user_resize` migrations that have not cut over
   * to `rolling_back` (rollback_user_resizes), so the engine deletes any
   * candidate machine. Returns the migration ids.
   */
  rollBackOpenResizes(userId: string): Promise<string[]>;
}

export interface SpendCapDeps {
  store: SpendCapStore;
  providers: (kind: ProviderKind, placementId?: string | null) => Pick<ManagedComputeProvider, "stopInstance">;
  now: () => Date;
  log: (fields: Record<string, unknown>) => void;
}

export interface SpendCapResult {
  /** Users sent the 80% notice in this run. */
  warned: string[];
  /** Users sent the 100% notice in this run. */
  capped: string[];
  /** Instances stopped in this run. */
  stopped: string[];
  failed: Array<{ instanceId: string; error: string }>;
  /** Open resizes sent to rollback in this run. */
  rolledBackMigrations: string[];
}

/** The actions that start or grow a machine; refused while capped. */
export const SPEND_CAP_GATED_ACTIONS: ReadonlySet<string> = new Set(["ensure", "wake", "resize"]);

const RUNNING_STATES = ["ready", "waking"];

const usd = (n: number) => `$${n.toFixed(2)}`;

export function spendCapRefusal(
  action: unknown,
  status: SpendStatus | null,
): { status: 402; body: Record<string, unknown> } | null {
  if (typeof action !== "string" || !SPEND_CAP_GATED_ACTIONS.has(action)) return null;
  if (!status || status.state !== "capped") return null;
  return {
    status: 402,
    body: {
      code: "spend_cap_reached",
      error: `You reached your monthly cloud spend cap of ${usd(status.cap_usd)}. Raise the cap to keep working.`,
      cap_usd: status.cap_usd,
      spent_usd: status.spent_usd,
    },
  };
}

// The cap window is the owner's Stripe billing period, or the calendar
// month without one (046_placements_and_usage.sql). A calendar month keeps its
// "YYYY-MM" key; a Stripe period is keyed by its start hour.
function noticePeriod(windowStart: string, now: Date): string {
  const start = new Date(windowStart);
  if (Number.isNaN(start.getTime())) return now.toISOString().slice(0, 7);
  const iso = start.toISOString();
  return iso.slice(7, 19) === "-01T00:00:00" ? iso.slice(0, 7) : iso.slice(0, 13);
}

function notice(c: SpendCapCandidate, now: Date): SpendNotice {
  const period = noticePeriod(c.month_start, now);
  if (c.state === "capped") {
    return {
      userId: c.user_id,
      kind: "spend_cap_reached",
      period,
      title: "Cloud spend cap reached",
      body: `You spent ${usd(c.spent_usd)} of your ${usd(c.cap_usd)} monthly cap. ` +
        "Your cloud workspaces are stopped until you raise the cap or the month ends.",
    };
  }
  return {
    userId: c.user_id,
    kind: "spend_cap_warning",
    period,
    title: "Cloud spend at 80% of your cap",
    body: `You spent ${usd(c.spent_usd)} of your ${usd(c.cap_usd)} monthly cap. ` +
      "Workspaces stop when you reach the cap.",
  };
}

export async function enforceSpendCaps(deps: SpendCapDeps): Promise<SpendCapResult> {
  const now = deps.now();
  const result: SpendCapResult = { warned: [], capped: [], stopped: [], failed: [], rolledBackMigrations: [] };

  for (const c of await deps.store.candidates(now)) {
    if (await deps.store.notify(notice(c, now))) {
      (c.state === "capped" ? result.capped : result.warned).push(c.user_id);
    }
    if (c.state !== "capped") continue;
    // A resize builds a second machine; stop that too. The engine also
    // refuses cutover while the owner is capped.
    try {
      result.rolledBackMigrations.push(...await deps.store.rollBackOpenResizes(c.user_id));
    } catch (err) {
      deps.log({ operation: "spend_cap_rollback_failed", user_id: c.user_id, error: (err as Error).message });
    }
    for (const w of await deps.store.runningWorkspaces(c.user_id)) {
      try {
        await stopWorkspace(deps.providers, w, deps.store.markStopped);
        result.stopped.push(w.instance_id);
      } catch (err) {
        const error = err instanceof Error ? err.message : String(err);
        result.failed.push({ instanceId: w.instance_id, error });
        deps.log({ operation: "spend_cap_stop_failed", instance_id: w.instance_id, error });
      }
    }
  }

  deps.log({
    operation: "spend_cap_complete",
    warned: result.warned.length,
    capped: result.capped.length,
    stopped: result.stopped.length,
    failed: result.failed.length,
  });
  return result;
}

/** get_spend_status_for() through the service-role client. */
export async function spendStatusFor(supabase: SupabaseClient, userId: string): Promise<SpendStatus> {
  const data = await rpc(supabase, "get_spend_status_for", { p_user_id: userId });
  const s = data as Record<string, unknown>;
  return {
    cap_usd: Number(s.cap_usd),
    spent_usd: Number(s.spent_usd),
    month_start: String(s.month_start),
    state: s.state as SpendState,
  };
}

/** SpendCapStore over the service-role client. */
export function supabaseSpendCapStore(supabase: SupabaseClient): SpendCapStore {
  return {
    async candidates(now) {
      const data = await rpc(supabase, "spend_cap_candidates", { p_now: now.toISOString() });
      return ((data ?? []) as Record<string, unknown>[]).map((r) => ({
        user_id: r.user_id as string,
        cap_usd: Number(r.cap_usd),
        spent_usd: Number(r.spent_usd),
        month_start: r.month_start as string,
        state: r.state as "warning" | "capped",
      }));
    },
    async notify(n) {
      return (await rpc(supabase, "record_user_notification", {
        p_user_id: n.userId,
        p_kind: n.kind,
        p_period: n.period,
        p_title: n.title,
        p_body: n.body,
      })) === true;
    },
    async runningWorkspaces(userId) {
      const data = must(await supabase
        .from("remote_instances")
        .select("id, provider_kind, provider_resource_id, active_placement_id")
        .eq("owner_user_id", userId)
        .in("status", RUNNING_STATES), "list running workspaces");
      return (data ?? []).map((r) => ({
        instance_id: r.id as string,
        provider_kind: r.provider_kind as ProviderKind,
        provider_resource_id: (r.provider_resource_id as string | null) ?? null,
        active_placement_id: (r.active_placement_id as string | null) ?? null,
      }));
    },
    async rollBackOpenResizes(userId) {
      const data = await rpc(supabase, "rollback_user_resizes", { p_user_id: userId });
      return ((data ?? []) as Array<{ migration_id: string }>).map((r) => r.migration_id);
    },
    async markStopped(instanceId, activePlacementId) {
      if (activePlacementId) {
        // set_placement_state closes the running interval and updates the instance.
        await setPlacementState(supabase, activePlacementId, "suspended");
        return;
      }
      must(await supabase
        .from("remote_instances")
        .update({ status: "suspended", updated_at: new Date().toISOString() })
        .eq("id", instanceId)
        .in("status", RUNNING_STATES), "mark instance stopped");
    },
  };
}
