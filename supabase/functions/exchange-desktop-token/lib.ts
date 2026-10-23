// Pure logic for exchange-desktop-token, kept free of Deno globals so vitest
// can exercise it.

export type DesktopSession = {
  access_token: string;
  refresh_token: string;
  expires_in: number;
  user: unknown;
};

export type ExchangeDeps = {
  /** Atomically marks the token used and returns its user, or null. */
  consumeToken: (token: string, now: Date) => Promise<string | null>;
  createSession: (userId: string) => Promise<DesktopSession | null>;
  now: () => Date;
};

export type ExchangeResult = { status: number; body: unknown };

/** The slice of the PostgREST builder `consumeDesktopToken` uses. */
export type TokenTable = {
  update(patch: { used: boolean }): TokenTable;
  eq(column: string, value: unknown): TokenTable;
  gt(column: string, value: unknown): TokenTable;
  select(columns: string): TokenTable;
  maybeSingle(): PromiseLike<{
    data: { user_id: string } | null;
    error: { message: string } | null;
  }>;
};

/**
 * Spends a desktop token in one UPDATE … RETURNING, so two concurrent
 * exchanges cannot both succeed. Unknown, used and expired tokens all
 * return null.
 */
export async function consumeDesktopToken(
  table: TokenTable,
  token: string,
  now: Date,
): Promise<string | null> {
  const { data, error } = await table
    .update({ used: true })
    .eq("token", token)
    .eq("used", false)
    .gt("expires_at", now.toISOString())
    .select("user_id")
    .maybeSingle();
  if (error) throw new Error(error.message);
  return data?.user_id ?? null;
}

export async function exchangeDesktopToken(
  deps: ExchangeDeps,
  body: unknown,
): Promise<ExchangeResult> {
  const token = (body as { token?: unknown } | null)?.token;
  if (typeof token !== "string" || token === "") {
    return { status: 400, body: { error: "Token is required" } };
  }

  try {
    const userId = await deps.consumeToken(token, deps.now());
    if (!userId) {
      return { status: 401, body: { error: "Invalid or expired token" } };
    }
    const session = await deps.createSession(userId);
    if (!session) {
      return { status: 500, body: { error: "Failed to create session" } };
    }
    return { status: 200, body: session };
  } catch (err) {
    console.error("[exchange-desktop-token]", err);
    return { status: 500, body: { error: "Internal error" } };
  }
}
