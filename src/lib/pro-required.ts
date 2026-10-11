// The server refuses a Pro feature with HTTP 402 and code `pro_required`
// (prds/billing-and-teams.md, "Enforcement"). These helpers let the app show
// its "Upgrade to Pro" affordance in place of an error. Keep in sync with
// web/src/lib/pro-required.ts.

export const PRO_REQUIRED_CODE = "pro_required";

// The SQLSTATE set_merge_queue_enabled raises. PostgREST answers it with
// HTTP 402 and passes the code through.
const POSTGREST_PRO_REQUIRED = "PT402";

// How `remoteFunctionError` starts its message, which the cloud workspace
// flows keep as a string.
const STORED_PREFIX = `[${PRO_REQUIRED_CODE}] `;

/**
 * True for a refusal because the user has no Pro: a PostgREST error from an
 * RPC, a `supabase.functions.invoke` error, a `RemoteFunctionError`, or the
 * message a `RemoteFunctionError` was stored as.
 */
export function isProRequiredError(error: unknown): boolean {
  if (typeof error === "string") return error.startsWith(STORED_PREFIX);
  if (typeof error !== "object" || error === null) return false;
  const { code, hint, context } = error as {
    code?: unknown;
    hint?: unknown;
    context?: unknown;
  };
  if (
    code === PRO_REQUIRED_CODE ||
    code === POSTGREST_PRO_REQUIRED ||
    hint === PRO_REQUIRED_CODE
  ) {
    return true;
  }
  // FunctionsHttpError keeps the raw response. Only Pro gates answer 402.
  return context instanceof Response && context.status === 402;
}

/** The server's sentence from a stored `[pro_required] …` message. */
export function proRequiredMessage(message: string): string {
  if (!message.startsWith(STORED_PREFIX)) return message;
  return message.slice(STORED_PREFIX.length).split("\n")[0];
}
