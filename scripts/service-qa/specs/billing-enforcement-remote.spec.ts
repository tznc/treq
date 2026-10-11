/**
 * Pro enforcement on managed cloud workspaces (`remote-instance`) and the
 * lapse sweep (remote-admin `sweep_lapsed_instances`) against the local
 * database.
 *
 * Runs `remote-instance/lib.ts` and `_shared/remote/lapse-sweep.ts` in this
 * process with the service-role client, so has_pro, remote_instances and the
 * lapse functions in 027_billing_enforcement.sql are the real ones. The
 * provider is the in-memory Sprites stub, as with TREQ_REMOTE_SPRITES_STUB=1.
 * Pro comes from the billing write functions (../billing.ts).
 */
import { randomUUID } from "node:crypto";
import { expect, it } from "vitest";
import {
  PRO_REQUIRED_MESSAGES,
  userHasPro,
} from "../../../supabase/functions/_shared/billing/entitlement";
import {
  supabaseLapseSweepStore,
  sweepLapsedInstances,
} from "../../../supabase/functions/_shared/remote/lapse-sweep";
import { StubSpritesProvider } from "../../../supabase/functions/_shared/remote/stub-sprites-adapter";
import { handleRemoteInstanceAction } from "../../../supabase/functions/remote-instance/lib";
import { clearBilling, endPro, grantPro } from "../billing";
import { getServiceClient } from "../clients";
import { recordOutcome } from "../record";
import { createTestUser, deleteTestUser } from "../seed";

const admin = getServiceClient();

async function call(userId: string, action: string, body: Record<string, unknown> = {}) {
  const response = await handleRemoteInstanceAction(
    { action, idempotency_key: `${action}-${randomUUID()}`, ...body },
    {
      supabase: admin,
      ownerUserId: userId,
      correlationId: `sqa-${randomUUID()}`,
      hasPro: () => userHasPro(admin, userId),
      stub: true,
      provider: () => new StubSpritesProvider(),
    },
  );
  return {
    status: response.status,
    json: (await response.json()) as Record<string, unknown>,
  };
}

function sweep() {
  return sweepLapsedInstances({
    store: supabaseLapseSweepStore(admin, `sqa-sweep-${randomUUID()}`),
    provider: new StubSpritesProvider(),
    now: () => new Date(),
    log: () => {},
  });
}

async function instanceRow(userId: string) {
  const { data, error } = await admin
    .from("remote_instances")
    .select("id, status, lapsed_at, endpoint_id")
    .eq("owner_user_id", userId)
    .order("created_at", { ascending: false })
    .limit(1)
    .single();
  if (error) throw new Error(error.message);
  return data as { id: string; status: string; lapsed_at: string | null; endpoint_id: string | null };
}

async function backdateLapse(instanceId: string, days: number) {
  const { error } = await admin
    .from("remote_instances")
    .update({ lapsed_at: new Date(Date.now() - days * 86_400_000).toISOString() })
    .eq("id", instanceId);
  if (error) throw new Error(error.message);
}

it("requires Pro to start a cloud workspace and keeps status and delete open", async () => {
  const free = await createTestUser();
  const lapsing = await createTestUser();
  try {
    const refusal = {
      status: 402,
      json: expect.objectContaining({
        error: PRO_REQUIRED_MESSAGES.cloudWorkspace,
        code: "pro_required",
      }),
    };
    expect(await call(free.user.id, "ensure")).toEqual(refusal);
    expect(await call(free.user.id, "status")).toEqual({
      status: 200,
      json: expect.objectContaining({ instance: null }),
    });
    const freeRows = await admin
      .from("remote_instances")
      .select("id")
      .eq("owner_user_id", free.user.id);
    expect(freeRows.data).toEqual([]);

    const grant = await grantPro(admin, lapsing.user.id);
    const ensured = await call(lapsing.user.id, "ensure");
    expect(ensured.status).toBe(200);
    expect(ensured.json.status).toBe("succeeded");
    const instanceId = (ensured.json.instance as { instance_id: string }).instance_id;
    expect((await call(lapsing.user.id, "wake", { instance_id: instanceId })).status).toBe(200);
    expect((await call(lapsing.user.id, "reprovision", { instance_id: instanceId })).status).toBe(200);

    await recordOutcome("billing-enforcement-remote-01-gate", {
      expectations: [
        "A Free user's ensure answers 402 pro_required and creates no remote_instances row, while status answers 200.",
        "A Pro user's ensure, wake and reprovision each answer 200 and provision one instance.",
      ],
      details: { instanceId, ensured: ensured.json.status },
    });

    await endPro(admin, grant);
    for (const action of ["ensure", "wake", "reprovision"]) {
      expect(await call(lapsing.user.id, action, { instance_id: instanceId })).toEqual(refusal);
    }
    const status = await call(lapsing.user.id, "status");
    expect(status.status).toBe(200);
    expect((status.json.instance as { instance_id: string }).instance_id).toBe(instanceId);
    const deleted = await call(lapsing.user.id, "delete", { instance_id: instanceId });
    expect(deleted.status).toBe(200);
    expect((await instanceRow(lapsing.user.id)).status).toBe("deleted");

    await recordOutcome("billing-enforcement-remote-02-lapsed-actions", {
      expectations: [
        "Once the subscription is deleted, ensure, wake and reprovision answer 402 pro_required.",
        "status still returns the user's instance, and delete still tears it down (status 'deleted').",
      ],
      details: { statusCode: status.status, deleteCode: deleted.status },
    });
  } finally {
    await clearBilling(admin, [free.user.id, lapsing.user.id]);
    await deleteTestUser(admin, free.user.id);
    await deleteTestUser(admin, lapsing.user.id);
  }
}, 120_000);

it("stops a lapsed cloud workspace, deletes it after 30 days, and spares one whose owner came back", async () => {
  const leaver = await createTestUser();
  const returner = await createTestUser();
  try {
    const leaverGrant = await grantPro(admin, leaver.user.id);
    const returnerGrant = await grantPro(admin, returner.user.id);
    expect((await call(leaver.user.id, "ensure")).status).toBe(200);
    expect((await call(returner.user.id, "ensure")).status).toBe(200);
    await endPro(admin, leaverGrant);
    await endPro(admin, returnerGrant);

    // First sweep: both lapse and stop.
    const first = await sweep();
    const leaverRow = await instanceRow(leaver.user.id);
    const returnerRow = await instanceRow(returner.user.id);
    expect(first.stopped).toEqual(expect.arrayContaining([leaverRow.id, returnerRow.id]));
    expect(leaverRow.status).toBe("suspended");
    expect(leaverRow.lapsed_at).not.toBeNull();
    expect(returnerRow.status).toBe("suspended");

    // A second sweep inside the grace period keeps both.
    const second = await sweep();
    expect(second.kept).toEqual(expect.arrayContaining([leaverRow.id, returnerRow.id]));
    expect((await instanceRow(leaver.user.id)).lapsed_at).toBe(leaverRow.lapsed_at);

    await recordOutcome("billing-enforcement-remote-03-stop", {
      expectations: [
        "The first sweep after Pro ends records lapsed_at and moves each ready instance to 'suspended'.",
        "A second sweep inside the 30 days keeps both instances and the original lapsed_at.",
      ],
      details: { first, leaverRow },
    });

    // The returner subscribes again. The subscription write clears the lapse.
    await grantPro(admin, returner.user.id, {
      customerId: returnerGrant.customerId,
    });
    expect((await instanceRow(returner.user.id)).lapsed_at).toBeNull();

    // 31 days on for the leaver; a stale 31-day lapse for the returner.
    await backdateLapse(leaverRow.id, 31);
    await backdateLapse(returnerRow.id, 31);
    const third = await sweep();

    const leaverAfter = await instanceRow(leaver.user.id);
    expect(third.deleted).toContain(leaverRow.id);
    expect(leaverAfter.status).toBe("deleted");
    expect(leaverAfter.endpoint_id).toBeNull();
    const audit = await admin
      .from("remote_audit_events")
      .select("event_type")
      .eq("instance_id", leaverRow.id)
      .in("event_type", ["instance_lapse_stopped", "instance_lapse_deleted"]);
    expect((audit.data ?? []).map((r) => r.event_type).sort()).toEqual([
      "instance_lapse_deleted",
      "instance_lapse_stopped",
    ]);

    const returnerAfter = await instanceRow(returner.user.id);
    expect(third.restored).toContain(returnerRow.id);
    expect(third.deleted).not.toContain(returnerRow.id);
    expect(returnerAfter.status).toBe("suspended");
    expect(returnerAfter.lapsed_at).toBeNull();
    expect((await call(returner.user.id, "wake", { instance_id: returnerRow.id })).status).toBe(200);

    await recordOutcome("billing-enforcement-remote-04-delete-or-restore", {
      expectations: [
        "After 31 days without Pro the sweep deletes the instance (status 'deleted', endpoint cleared) and audits the stop and the delete.",
        "An owner who subscribed again keeps the instance even with a 31-day-old lapse: the sweep clears lapsed_at instead.",
        "The returning owner can wake the instance again.",
      ],
      details: { third, leaverAfter, returnerAfter },
    });
  } finally {
    await clearBilling(admin, [leaver.user.id, returner.user.id]);
    await deleteTestUser(admin, leaver.user.id);
    await deleteTestUser(admin, returner.user.id);
  }
}, 120_000);
