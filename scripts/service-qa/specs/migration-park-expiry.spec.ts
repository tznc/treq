/**
 * The hourly run (`remote-admin` `advance_migrations`) against the local
 * database: a resize parked on a failed re-clone past its deadline is
 * rolled back and its stopped new machine is marked deleted, while one
 * still inside its deadline stays parked.
 *
 * Runs `operator-controls.ts` in this process with the service-role client
 * and the local provider, as migration-cancel.spec.ts runs its handler.
 */
import { randomUUID } from "node:crypto";
import { afterAll, beforeAll, expect, it } from "vitest";
import { LocalProvider } from "../../../supabase/functions/_shared/remote/local-provider";
import { advanceMigrations } from "../../../supabase/functions/_shared/remote/operator-controls";
import { getServiceClient } from "../clients";
import { recordOutcome } from "../record";
import { createTestUser, deleteTestUser } from "../seed";

const admin = getServiceClient();

function must<T>(r: { data: T; error: unknown }): T {
  if (r.error) throw r.error;
  return r.data;
}

const users: string[] = [];

async function parkedResize(
  deadline: Date,
): Promise<{ migrationId: string; targetId: string }> {
  const { user } = await createTestUser();
  users.push(user.id);
  const instanceId = randomUUID();
  const sourceId = randomUUID();
  const targetId = randomUUID();
  must(
    await admin.from("remote_instances").insert({
      id: instanceId,
      owner_user_id: user.id,
      provider_kind: "boat",
      region: "us_east",
      size_preset: "small",
      status: "ready",
      tier_id: "s",
      machine_type_id: "boat:small",
      provider_resource_id: `bx_src_${instanceId}`,
      fence: 0,
    }),
  );
  must(
    await admin.from("remote_instance_placements").insert([
      {
        id: sourceId,
        instance_id: instanceId,
        owner_user_id: user.id,
        provider_kind: "boat",
        machine_type_id: "boat:small",
        region: "us_east",
        state: "ready",
        role: "active",
        provider_resource_id: `bx_src_${instanceId}`,
      },
      {
        id: targetId,
        instance_id: instanceId,
        owner_user_id: user.id,
        provider_kind: "boat",
        machine_type_id: "boat:large",
        region: "us_east",
        state: "suspended",
        role: "candidate",
        provider_resource_id: `bx_tgt_${instanceId}`,
      },
    ]),
  );
  must(
    await admin
      .from("remote_instances")
      .update({ active_placement_id: sourceId })
      .eq("id", instanceId),
  );
  const row = must(
    await admin
      .from("sandbox_migrations")
      .insert({
        instance_id: instanceId,
        owner_user_id: user.id,
        source_placement_id: sourceId,
        target_placement_id: targetId,
        target_machine_type_id: "boat:large",
        reason: "user_resize",
        state: "awaiting_ack",
        deadline: deadline.toISOString(),
      })
      .select("id")
      .single(),
  ) as { id: string };
  return { migrationId: row.id, targetId };
}

let expired: { migrationId: string; targetId: string };
let fresh: { migrationId: string; targetId: string };

beforeAll(async () => {
  expired = await parkedResize(new Date(Date.now() - 60_000));
  fresh = await parkedResize(new Date(Date.now() + 3600_000));
});

afterAll(async () => {
  for (const id of users) await deleteTestUser(admin, id);
});

it("rolls back an expired clone-failure park and deletes its new machine", async () => {
  const result = await advanceMigrations(
    {
      supabase: admin,
      providers: (kind) => new LocalProvider(kind),
      stub: true,
      correlationId: `sqa-${randomUUID()}`,
      operatorId: "service-qa",
    },
    { migration_id: undefined },
  );
  expect(result.expired).toContain(expired.migrationId);
  expect(result.expired).not.toContain(fresh.migrationId);

  const m = must(
    await admin
      .from("sandbox_migrations")
      .select("state, error")
      .eq("id", expired.migrationId)
      .single(),
  ) as { state: string; error: { reason?: string } | null };
  expect(m.state).toBe("rolled_back");
  const target = must(
    await admin
      .from("remote_instance_placements")
      .select("state, role")
      .eq("id", expired.targetId)
      .single(),
  ) as { state: string; role: string };
  expect(target).toEqual({ state: "deleted", role: "dead" });

  const f = must(
    await admin
      .from("sandbox_migrations")
      .select("state")
      .eq("id", fresh.migrationId)
      .single(),
  ) as { state: string };
  expect(f.state).toBe("awaiting_ack");

  await recordOutcome("migration-park-expiry-01-expired-park-rolled-back", {
    expectations: [
      "advance_migrations reports the expired clone-failure park in `expired` and not the unexpired one.",
      "The expired migration ends rolled_back and its new machine's placement is deleted and dead.",
      "A park still inside its deadline stays awaiting_ack.",
    ],
    details: {
      expired,
      fresh,
      result: { expired: result.expired },
      migration: m,
      target,
    },
  });
}, 60000);
