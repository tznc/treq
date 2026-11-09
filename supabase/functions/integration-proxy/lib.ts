// Request logic for the integration-proxy Edge Function, kept free of Deno
// and Supabase imports so it runs under the repo's unit tests.

export type Caller = { kind: "user"; userId: string };

export type Result = { status: number; body: unknown };

export interface AuthDeps {
  /** The user id for a user's Supabase JWT, or null when it is not valid. */
  userFromJwt(jwt: string): Promise<string | null>;
}

/** Resolves the Authorization header to a caller, or null when it is not valid. */
export async function resolveCaller(
  authorization: string | undefined,
  deps: AuthDeps,
): Promise<Caller | null> {
  const jwt = (authorization ?? "").replace(/^Bearer\s+/i, "");
  if (jwt.split(".").length !== 3) return null;
  const userId = await deps.userFromJwt(jwt);
  return userId ? { kind: "user", userId } : null;
}
