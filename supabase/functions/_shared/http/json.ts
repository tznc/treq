// JSON responses for the Edge Functions a signed-in user calls.

export const corsHeaders = {
  "Access-Control-Allow-Origin": "*",
  "Access-Control-Allow-Headers": "authorization, x-client-info, apikey, content-type",
};

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
