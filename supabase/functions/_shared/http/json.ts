// JSON responses for the Edge Functions a signed-in user calls.

export const corsHeaders = {
  "Access-Control-Allow-Origin": "*",
  "Access-Control-Allow-Headers": "authorization, x-client-info, apikey, content-type",
};

/**
 * A request or response body parsed as JSON, or null when it is not JSON (an
 * empty or malformed body). Any other failure, such as a dropped stream, throws.
 */
export async function jsonBodyOrNull(message: Request | Response): Promise<unknown> {
  try {
    return await message.json();
  } catch (err) {
    if (err instanceof SyntaxError) return null;
    throw err;
  }
}

/** With a correlation id, it is echoed in the body and the x-correlation-id header. */
export function json(body: unknown, status = 200, correlationId?: string): Response {
  return new Response(JSON.stringify(correlationId ? { ...(body as object), correlation_id: correlationId } : body), {
    status,
    headers: {
      ...corsHeaders,
      "Content-Type": "application/json",
      ...(correlationId ? { "x-correlation-id": correlationId } : {}),
    },
  });
}
