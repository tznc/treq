// Server-side Pro checks for the cloud features (prds/billing-and-teams.md,
// "Enforcement"). Every gate asks Postgres: has_pro for a user, and
// installation_has_pro for a GitHub App installation. The app's own Pro
// gates only decide what it shows.
//
// A refused request answers HTTP 402 with `code: "pro_required"`, which the
// desktop app and the web dashboard turn into their "Upgrade to Pro"
// affordance instead of an error. When the check itself fails, these helpers
// throw, so the caller answers 500 rather than letting the request through.

export const PRO_REQUIRED_CODE = "pro_required";

export type ProRequiredBody = { error: string; code: typeof PRO_REQUIRED_CODE };

export function proRequired(message: string): {
  status: 402;
  body: ProRequiredBody;
} {
  return { status: 402, body: { error: message, code: PRO_REQUIRED_CODE } };
}

/**
 * For a function with several actions, some of them Pro: the 402 answer
 * when `action` is gated and the caller has no Pro, otherwise null. Actions
 * outside `gated` never check entitlement.
 */
export async function refuseGatedAction(
  action: unknown,
  gated: ReadonlySet<string>,
  hasPro: () => Promise<boolean>,
  message: string,
): Promise<ReturnType<typeof proRequired> | null> {
  if (typeof action !== "string" || !gated.has(action)) return null;
  return (await hasPro()) ? null : proRequired(message);
}

/** What each gated feature says when it refuses a Free user. */
export const PRO_REQUIRED_MESSAGES = {
  githubApp:
    "The Treq GitHub App needs Pro. Start a trial or upgrade on your treq.dev dashboard.",
  mergeQueue: "The merge queue needs Pro.",
  linearOAuth:
    "Linear OAuth needs Pro. Add a personal Linear API key in the repository's Linear settings to keep using Linear on the Free plan.",
  cloudWorkspace:
    "Cloud workspaces need Pro. You can still check the status of your cloud workspace or delete it.",
} as const;

type RpcResult = { data: unknown; error: { message: string } | null };

/** A service-role Supabase client, or anything with the same `rpc`. */
export interface RpcClient {
  rpc(fn: string, args: Record<string, unknown>): PromiseLike<RpcResult>;
}

/** has_pro for a user. The client must use the service role. */
export async function userHasPro(
  client: RpcClient,
  userId: string,
): Promise<boolean> {
  const { data, error } = await client.rpc("has_pro", { p_user_id: userId });
  if (error) throw new Error(`has_pro failed: ${error.message}`);
  return data === true;
}

/** installation_has_pro for a GitHub App installation (service role only). */
export async function installationHasPro(
  client: RpcClient,
  installationId: number,
): Promise<boolean> {
  const { data, error } = await client.rpc("installation_has_pro", {
    p_installation_id: installationId,
  });
  if (error) throw new Error(`installation_has_pro failed: ${error.message}`);
  return data === true;
}
