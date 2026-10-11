/**
 * Per-spec fixture: signed-in email/password user + linked GitHub repo +
 * optional merge-queue rows. Always tear down Auth user and seeded rows so
 * specs do not leak state across the local Supabase CLI stack.
 */
import type { SupabaseClient, User } from "@supabase/supabase-js";
import { clearBilling, grantPro, type ProGrant } from "./billing";
import { getAnonClient, getServiceClient } from "./clients";
import { FEATURES } from "./features";
import {
  createTestUser,
  deleteTestUser,
  enqueueBranch,
  linkGithubRepo,
  type LinkedRepo,
  type TestUser,
} from "./seed";

export type MergeQueueFixture = {
  admin: SupabaseClient;
  /** Anon client with an active email/password session. */
  client: SupabaseClient;
  user: User;
  email: string;
  password: string;
  linked: LinkedRepo;
  /** The user's Pro subscription; null for a fixture created with `pro: false`. */
  pro: ProGrant | null;
  /** Enable the merge queue config row for the linked repo's default branch. */
  enableQueue: () => Promise<void>;
  /**
   * Insert a queued entry (service-role) so desktop RPCs can read status
   * without calling GitHub via enqueue-workspace.
   */
  seedQueuedEntry: (opts: {
    branchName: string;
    prNumber: number;
    prSha?: string;
    position?: number;
    targetBranch?: string;
  }) => Promise<{ queueId: string; entryId: string }>;
  teardown: () => Promise<void>;
};

/**
 * Create a signed-in email/password user (feature flag overridden on) and a
 * linked GitHub repo. The user has Pro unless `pro: false`, since turning
 * the merge queue on and running it require Pro (027_billing_enforcement.sql).
 * Caller must `await fixture.teardown()` — prefer `withMergeQueueFixture`.
 */
export async function setupMergeQueueFixture(opts?: {
  fullName?: string;
  pro?: boolean;
}): Promise<MergeQueueFixture> {
  if (!FEATURES.emailSignup) {
    throw new Error("emailSignup feature flag must be true in service-qa");
  }

  const admin = getServiceClient();
  const testUser: TestUser = await createTestUser();
  const pro =
    opts?.pro === false ? null : await grantPro(admin, testUser.user.id);
  const linked = await linkGithubRepo(admin, testUser.user.id, {
    fullName: opts?.fullName,
  });

  const client = getAnonClient();
  const { data: signIn, error: signInError } =
    await client.auth.signInWithPassword({
      email: testUser.email,
      password: testUser.password,
    });
  if (signInError || !signIn.session) {
    await clearBilling(admin, [testUser.user.id]);
    await deleteTestUser(admin, testUser.user.id);
    throw new Error(
      `email/password sign-in failed: ${signInError?.message ?? "no session"}`,
    );
  }

  let tornDown = false;
  const teardown = async () => {
    if (tornDown) return;
    tornDown = true;
    try {
      await client.auth.signOut();
    } catch {
      // ignore
    }
    // Cascades: installation → repos → configs/queues/entries (FK on delete).
    await admin
      .from("github_app_installations")
      .delete()
      .eq("id", linked.installationId);
    await clearBilling(admin, [testUser.user.id]);
    await deleteTestUser(admin, testUser.user.id);
  };

  const enableQueue = async () => {
    const { error } = await client.rpc("set_merge_queue_enabled", {
      p_repo_full_name: linked.fullName,
      p_enabled: true,
    });
    if (error) throw new Error(`enableQueue: ${error.message}`);
  };

  const seedQueuedEntry: MergeQueueFixture["seedQueuedEntry"] = async ({
    branchName,
    prNumber,
    prSha = "abc123def456",
    position = 1,
    targetBranch = linked.defaultBranch,
  }) => {
    return enqueueBranch(admin, {
      repoId: linked.repoId,
      branchName,
      prNumber,
      prSha,
      position,
      targetBranch,
    });
  };

  return {
    admin,
    client,
    user: testUser.user,
    email: testUser.email,
    password: testUser.password,
    linked,
    pro,
    enableQueue,
    seedQueuedEntry,
    teardown,
  };
}

/** Run `fn` with a fresh fixture; always teardown even on assertion failure. */
export async function withMergeQueueFixture<T>(
  fn: (fixture: MergeQueueFixture) => Promise<T>,
  opts?: { fullName?: string; pro?: boolean },
): Promise<T> {
  const fixture = await setupMergeQueueFixture(opts);
  try {
    return await fn(fixture);
  } finally {
    await fixture.teardown();
  }
}
