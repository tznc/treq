/**
 * Declining a parked migration (`remote-instance` `cancel_migration`)
 * against the local database: the owner cancels a migration waiting in
 * `awaiting_ack`, another user cannot, a running migration is refused, and
 * the instance can be migrated again afterwards (the one-open index frees).
 *
 * Runs `remote-instance/lib.ts` in this process with the service-role
 * client, as billing-enforcement-remote.spec.ts does; the user is Free, so
 * this also proves the action is not Pro-gated.
 */
import { randomUUID } from "node:crypto";
import { afterAll, beforeAll, expect, it } from "@jest/globals";
import { userHasPro } from "../../../supabase/functions/_shared/billing/entitlement";
import { LocalProvider } from "../../../supabase/functions/_shared/remote/local-provider";
import { handleRemoteInstanceAction } from "../../../supabase/functions/remote-instance/lib";
import { getServiceClient } from "../clients";
import { recordOutcome } from "../record";
import { createTestUser, deleteTestUser } from "../seed";

const admin = getServiceClient();

async function call(userId: string, body: Record<string, unknown>) {
  const response = await handleRemoteInstanceAction(
    { action: "cancel_migration", idempotency_key: `cancel-${randomUUID()}`, ...body },
    {
      supabase: admin,
      ownerUserId: userId,
      correlationId: `sqa-${randomUUID()}`,
      hasPro: () => userHasPro(admin, userId),
      stub: true,
      provider: (kind) => new LocalProvider(kind),
    },
  );
  return { status: response.status, json: (await response.json()) as Record<string, unknown> };
}

function must<T>(r: { data: T; error: unknown }): T {
  if (r.error) throw r.error;
  return r.data;
}

let owner: { user: { id: string } };
let other: { user: { id: string } };
let instanceId: string;
let placementId: string;

async function seedMigration(state: string): Promise<string> {
  const row = must(
    await admin
      .from("sandbox_migrations")
      .insert({
        instance_id: instanceId,
        owner_user_id: owner.user.id,
        source_placement_id: placementId,
        target_machine_type_id: "boat:small",
        reason: "drain",
        state,
        preflight: { repos: [], requires_ack: true },
      })
      .select("id")
      .single(),
  ) as { id: string };
  return row.id;
}

async function migrationState(id: string): Promise<string> {
  return (must(await admin.from("sandbox_migrations").select("state").eq("id", id).single()) as { state: string }).state;
}

beforeAll(async () => {
  owner = await createTestUser();
  other = await createTestUser();
  instanceId = randomUUID();
  placementId = randomUUID();
  must(
    await admin.from("remote_instances").insert({
      id: instanceId,
      owner_user_id: owner.user.id,
      provider_kind: "boat",
      region: "us_east",
      size_preset: "small",
      status: "ready",
      tier_id: "s",
      machine_type_id: "boat:small",
      provider_resource_id: "bx_src",
    }),
  );
  must(
    await admin.from("remote_instance_placements").insert({
      id: placementId,
      instance_id: instanceId,
      owner_user_id: owner.user.id,
      provider_kind: "boat",
      machine_type_id: "boat:small",
      region: "us_east",
      state: "ready",
      role: "active",
      provider_resource_id: "bx_src",
    }),
  );
  must(await admin.from("remote_instances").update({ active_placement_id: placementId }).eq("id", instanceId));
}, 60000);

afterAll(async () => {
  await admin.from("sandbox_migrations").delete().eq("instance_id", instanceId);
  await admin.from("remote_instances").update({ active_placement_id: null }).eq("id", instanceId);
  await admin.from("remote_instance_placements").delete().eq("instance_id", instanceId);
  await admin.from("remote_audit_events").delete().eq("instance_id", instanceId);
  await admin.from("remote_instances").delete().eq("id", instanceId);
  await deleteTestUser(admin, owner.user.id);
  await deleteTestUser(admin, other.user.id);
}, 60000);

it("lets the owner decline a parked migration, and only the owner", async () => {
  const id = await seedMigration("awaiting_ack");

  const stranger = await call(other.user.id, { migration_id: id });
  expect(stranger.status).toBe(404);
  expect(await migrationState(id)).toBe("awaiting_ack");

  const key = `cancel-${randomUUID()}`;
  const first = await call(owner.user.id, { migration_id: id, idempotency_key: key });
  expect(first.status).toBe(200);
  expect(first.json.migration_state).toBe("cancelled");
  expect(await migrationState(id)).toBe("cancelled");
  const again = await call(owner.user.id, { migration_id: id, idempotency_key: key });
  expect(again.status).toBe(200);

  const audits = must(
    await admin.from("remote_audit_events").select("event_type").eq("instance_id", instanceId).eq("event_type", "migration_cancelled"),
  ) as unknown[];
  expect(audits).toHaveLength(1);

  // The one-open index is free again: a new migration can be created.
  const next = await seedMigration("requested");
  expect(next).toBeTruthy();

  await recordOutcome("migration-cancel-01-owner-declines-parked", {
    expectations: [
      "Another user's cancel_migration answers 404 and leaves the migration in awaiting_ack.",
      "The Free owner's cancel answers 200 and the row is cancelled; re-sending answers 200 with one migration_cancelled audit row.",
      "After the cancel, a new migration for the same instance can be inserted.",
    ],
    details: { strangerStatus: stranger.status, first: first.json, againStatus: again.status, audits: audits.length },
  });
}, 60000);

it("refuses to cancel a migration that is running", async () => {
  const running = (must(
    await admin.from("sandbox_migrations").select("id").eq("instance_id", instanceId).eq("state", "requested").single(),
  ) as { id: string }).id;
  must(await admin.from("sandbox_migrations").update({ state: "verifying" }).eq("id", running));
  const r = await call(owner.user.id, { migration_id: running });
  expect(r.status).toBe(409);
  expect(r.json.code).toBe("migration_not_parked");
  expect(await migrationState(running)).toBe("verifying");

  await recordOutcome("migration-cancel-02-running-refused", {
    expectations: ["cancel_migration on a verifying migration answers 409 migration_not_parked and leaves it verifying."],
    details: { response: r.json },
  });
}, 60000);
