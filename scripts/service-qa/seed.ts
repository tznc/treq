/**
 * Seed helpers for service-qa specs. Prefer these over hand-rolled inserts so
 * specs stay consistent with the migration schema and RLS expectations.
 *
 * Auth: email/password only. `createTestUser` uses the public `signUp` API
 * (same path the web UI uses when `emailSignup` is on). The service-qa
 * feature override forces that flag on — see `features.ts`.
 */
import type { SupabaseClient, User } from "@supabase/supabase-js";
import { randomBytes } from "node:crypto";
import { getAnonClient, getServiceClient } from "./clients";
import { FEATURES } from "./features";

export type TestUser = {
  user: User;
  email: string;
  password: string;
};

function uniqueEmail(prefix = "service-qa"): string {
  return `${prefix}-${randomBytes(6).toString("hex")}@example.com`;
}

function randomPassword(): string {
  return `Pw-${randomBytes(9).toString("base64url")}!a1`;
}

/**
 * Sign up a new user via Auth email/password (`signUp`), not the Admin API.
 * Local config has `enable_confirmations = false`, so signUp returns a session.
 */
export async function createTestUser(opts?: {
  email?: string;
  password?: string;
}): Promise<TestUser> {
  if (!FEATURES.emailSignup) {
    throw new Error(
      "createTestUser requires FEATURES.emailSignup=true (service-qa override).",
    );
  }

  const email = opts?.email ?? uniqueEmail();
  const password = opts?.password ?? randomPassword();
  const anon = getAnonClient();

  const { data, error } = await anon.auth.signUp({ email, password });
  if (error || !data.user) {
    throw new Error(`signUp failed: ${error?.message ?? "no user"}`);
  }

  // Drop the signup session so each spec signs in explicitly with password.
  await anon.auth.signOut();

  return { user: data.user, email, password };
}

/** Hard-delete a user (cascades profile + desktop tokens via FK). */
export async function deleteTestUser(
  admin: SupabaseClient,
  userId: string,
): Promise<void> {
  const { error } = await admin.auth.admin.deleteUser(userId);
  if (error) {
    throw new Error(`deleteTestUser failed: ${error.message}`);
  }
}

/**
 * Email/password sign-in returning a fresh anon client that holds the session.
 */
export async function signInWithEmailPassword(
  email: string,
  password: string,
): Promise<{ client: SupabaseClient; user: User }> {
  if (!FEATURES.emailSignup) {
    throw new Error(
      "signInWithEmailPassword requires FEATURES.emailSignup=true.",
    );
  }
  const client = getAnonClient();
  const { data, error } = await client.auth.signInWithPassword({
    email,
    password,
  });
  if (error || !data.session || !data.user) {
    throw new Error(
      `signInWithPassword failed: ${error?.message ?? "no session"}`,
    );
  }
  return { client, user: data.user };
}

export type LinkedRepo = {
  installationId: number;
  repoId: number;
  fullName: string;
  owner: string;
  name: string;
  defaultBranch: string;
};

/**
 * Insert a GitHub App installation linked to `userId` plus one repository.
 * Enough for desktop RPCs that join github_repositories → installations.
 */
export async function linkGithubRepo(
  admin: SupabaseClient,
  userId: string,
  opts?: {
    fullName?: string;
    installationId?: number;
    repoId?: number;
    defaultBranch?: string;
  },
): Promise<LinkedRepo> {
  const installationId =
    opts?.installationId ??
    Math.floor(Math.random() * 1_000_000_000) + 1_000_000;
  const repoId =
    opts?.repoId ?? Math.floor(Math.random() * 1_000_000_000) + 2_000_000;
  const fullName = opts?.fullName ?? `service-qa/repo-${repoId}`;
  const [owner, name] = fullName.split("/");
  const defaultBranch = opts?.defaultBranch ?? "main";

  const { error: installError } = await admin
    .from("github_app_installations")
    .insert({
      id: installationId,
      account_login: owner,
      account_type: "User",
      app_id: 1,
      linked_user_id: userId,
    });
  if (installError) {
    throw new Error(`linkGithubRepo install: ${installError.message}`);
  }

  const { error: repoError } = await admin.from("github_repositories").insert({
    id: repoId,
    installation_id: installationId,
    owner,
    name,
    full_name: fullName,
    private: false,
    default_branch: defaultBranch,
  });
  if (repoError) {
    throw new Error(`linkGithubRepo repo: ${repoError.message}`);
  }

  return {
    installationId,
    repoId,
    fullName,
    owner,
    name,
    defaultBranch,
  };
}

/**
 * Call create_desktop_token as the signed-in user (RLS / auth.uid() path the
 * web auth callback uses).
 */
export async function createDesktopToken(
  userClient: SupabaseClient,
): Promise<string> {
  const { data, error } = await userClient.rpc("create_desktop_token");
  if (error || typeof data !== "string" || data.length === 0) {
    throw new Error(
      `create_desktop_token failed: ${error?.message ?? "empty token"}`,
    );
  }
  return data;
}

/** Convenience: service client for teardown paths that need Admin Auth. */
export function adminClient(): SupabaseClient {
  return getServiceClient();
}
