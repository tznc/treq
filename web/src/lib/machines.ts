// The dashboard's view of the user's Treq Cloud machine, read and changed
// through the remote-instance Edge Function
// (supabase/functions/remote-instance/lib.ts). `status` and `delete` work
// without Pro. `ensure` and `wake` answer 402 `pro_required` without it.

import { functionErrorMessage } from "./billing";
import { proRequiredMessage } from "./pro-required";
import { supabase } from "./supabase";

/** The fields of the `status` reply's `instance` that the dashboard shows. */
export interface Machine {
  instance_id: string;
  status: string;
  region: string;
  size_preset: string;
  tier_id: string | null;
  created_at?: string;
  updated_at?: string;
}

export type MachineAction = "ensure" | "wake" | "delete";

export type StatusTone = "ok" | "busy" | "idle" | "error";

const STATUSES: Readonly<Record<string, { label: string; tone: StatusTone }>> = {
  unprovisioned: { label: "Starting", tone: "busy" },
  provisioning: { label: "Starting", tone: "busy" },
  bootstrapping: { label: "Starting", tone: "busy" },
  installing_access: { label: "Starting", tone: "busy" },
  verifying: { label: "Starting", tone: "busy" },
  ready: { label: "Running", tone: "ok" },
  suspended: { label: "Stopped", tone: "idle" },
  waking: { label: "Waking", tone: "busy" },
  reprovisioning: { label: "Repairing", tone: "busy" },
  degraded: { label: "Degraded", tone: "error" },
  failed: { label: "Failed", tone: "error" },
  deleting: { label: "Deleting", tone: "busy" },
};

export function machineStatus(status: string): { label: string; tone: StatusTone } {
  return STATUSES[status] ?? { label: status, tone: "idle" };
}

const TIERS: Readonly<Record<string, string>> = {
  s: "Small",
  m: "Medium",
  l: "Large",
  xl: "Extra large",
};
const SIZE_PRESETS: Readonly<Record<string, string>> = {
  small: "Small",
  medium: "Medium",
  large: "Large",
};

/** The size tier's name. Rows from before tiers only have a size preset. */
export function sizeLabel(machine: Pick<Machine, "tier_id" | "size_preset">): string {
  const tier = machine.tier_id ? TIERS[machine.tier_id] : undefined;
  if (tier) return tier;
  return SIZE_PRESETS[machine.size_preset] ?? machine.size_preset;
}

const REGIONS: Readonly<Record<string, string>> = {
  us_east: "US East",
  us_west: "US West",
  eu_west: "EU West",
  ap_southeast: "Asia Pacific",
};

export function regionLabel(code: string): string {
  return REGIONS[code] ?? code;
}

/**
 * What the user can do now. Delete always works. Create, Retry and Start
 * need Pro. Ensure picks up a failed or degraded machine again.
 */
export function machineActions(machine: Machine | null, isPro: boolean): MachineAction[] {
  if (!machine) return isPro ? ["ensure"] : [];
  if (machine.status === "deleting") return [];
  const actions: MachineAction[] = [];
  if (isPro && machine.status === "suspended") actions.push("wake");
  if (isPro && (machine.status === "failed" || machine.status === "degraded")) {
    actions.push("ensure");
  }
  actions.push("delete");
  return actions;
}

export function actionRequest(
  action: MachineAction,
  machine: Machine | null,
  idempotencyKey: string,
): Record<string, unknown> {
  if (action === "ensure") return { action, idempotency_key: idempotencyKey };
  return { action, instance_id: machine?.instance_id, idempotency_key: idempotencyKey };
}

/** The user's machine, or null when they have none. */
export async function fetchMachine(): Promise<Machine | null> {
  const { data, error } = await supabase.functions.invoke("remote-instance", {
    body: { action: "status" },
  });
  if (error) {
    throw new Error(await functionErrorMessage(error, "Could not load your machine."));
  }
  return ((data as { instance?: Machine | null } | null)?.instance ?? null);
}

/**
 * The stored error of the machine's latest lifecycle operation, when that
 * operation failed. The server scrubs it to a code and a short message.
 */
export async function fetchLastFailure(instanceId: string): Promise<string | null> {
  const { data } = await supabase
    .from("remote_instance_operations")
    .select("status, error_message")
    .eq("instance_id", instanceId)
    .in("operation_type", ["provision", "wake", "reprovision", "delete"])
    .order("created_at", { ascending: false })
    .limit(1);
  const latest = (data as { status: string; error_message: string | null }[] | null)?.[0];
  return latest?.status === "failed" ? latest.error_message : null;
}

/** An action the server refused. `needsPro` marks a 402 Pro refusal. */
export class MachineActionError extends Error {
  needsPro: boolean;
  constructor(message: string, needsPro: boolean) {
    super(message);
    this.needsPro = needsPro;
  }
}

const FAILED: Readonly<Record<MachineAction, string>> = {
  ensure: "The machine could not be created.",
  wake: "The machine could not be started.",
  delete: "The machine could not be deleted.",
};

export async function runMachineAction(
  action: MachineAction,
  machine: Machine | null,
): Promise<void> {
  const key = `${action}-web-${machine?.instance_id ?? "new"}-${globalThis.crypto.randomUUID()}`;
  const { error } = await supabase.functions.invoke("remote-instance", {
    body: actionRequest(action, machine, key),
  });
  if (!error) return;
  const needsPro = await proRequiredMessage(error);
  if (needsPro) throw new MachineActionError(needsPro, true);
  throw new MachineActionError(await functionErrorMessage(error, FAILED[action]), false);
}

export function formatMachineTime(value: string): string {
  return new Date(value).toLocaleString(undefined, { dateStyle: "medium", timeStyle: "short" });
}
