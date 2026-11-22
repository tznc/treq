// The server refuses a Pro feature with HTTP 402 and `code: "pro_required"`
// (prds/billing-and-teams.md, "Enforcement"). Keep in sync with the desktop
// app's src/lib/pro-required.ts.

/**
 * The server's message when a `supabase.functions.invoke` error is a Pro
 * refusal, or null for any other failure.
 */
export async function proRequiredMessage(error: unknown): Promise<string | null> {
  const context = (error as { context?: unknown } | null)?.context;
  if (!(context instanceof Response) || context.status !== 402) return null;
  const body = (await context
    .clone()
    .json()
    .catch((err: unknown) => {
      // A body that is not JSON carries no message.
      if (err instanceof SyntaxError) return null;
      throw err;
    })) as { error?: unknown } | null;
  return typeof body?.error === "string" ? body.error : "This feature needs Pro.";
}
