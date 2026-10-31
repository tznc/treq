// The remote-instance `resize` request (docs/design/sandbox-provider-adapter.md
// §6): validation and the decision on what to do with it. A resize always
// moves to a fresh machine of the tier's default type, so the only
// questions are whether anything changes, whether a switch is already
// running, and whether the user must acknowledge losing unpushed work.

import type { PreflightReport } from "./preflight.ts";

export interface ResizeRequest {
  tierId: string;
  idempotencyKey: string;
  instanceId: string | null;
  acknowledgedDataLoss: boolean;
}

export type Parsed<T> = { ok: true; value: T } | { ok: false; error: string };

const TIER_ID = /^[a-z0-9_]{1,32}$/;

export function parseResizeRequest(body: Record<string, unknown>): Parsed<ResizeRequest> {
  const key = body.idempotency_key;
  if (typeof key !== "string" || key.length === 0) return { ok: false, error: "idempotency_key is required" };
  const tier = body.tier_id;
  if (typeof tier !== "string" || tier.length === 0) return { ok: false, error: "tier_id is required" };
  if (!TIER_ID.test(tier)) return { ok: false, error: "tier_id is invalid" };
  const ack = body.acknowledged_data_loss;
  if (ack !== undefined && typeof ack !== "boolean") {
    return { ok: false, error: "acknowledged_data_loss must be a boolean" };
  }
  const instanceId = body.instance_id;
  if (instanceId !== undefined && instanceId !== null && typeof instanceId !== "string") {
    return { ok: false, error: "instance_id must be a string" };
  }
  return {
    ok: true,
    value: {
      tierId: tier,
      idempotencyKey: key,
      instanceId: instanceId ?? null,
      acknowledgedDataLoss: ack ?? false,
    },
  };
}

export type ResizeDecision =
  | { kind: "unchanged" }
  | { kind: "in_progress"; migrationId: string }
  | { kind: "needs_ack" }
  | { kind: "migrate" };

export function decideResize(p: {
  current: string | null;
  target: string;
  openMigrationId: string | null;
  preflight: PreflightReport | null;
  ack: boolean;
}): ResizeDecision {
  if (p.openMigrationId) return { kind: "in_progress", migrationId: p.openMigrationId };
  if (p.current === p.target) return { kind: "unchanged" };
  if (p.preflight?.requires_ack && !p.ack) return { kind: "needs_ack" };
  return { kind: "migrate" };
}
