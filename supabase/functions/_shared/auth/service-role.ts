// Gate for functions only schedulers call (Supabase Cron, the webhook
// nudge, the self-hosted cron). `verify_jwt = true` lets any valid JWT
// through, including the public anon key and every signed-in user's
// session, so these functions also require the service-role key itself.

/** True when `req` carries `Authorization: Bearer <serviceRoleKey>`. */
export function isServiceRoleRequest(
  req: Request,
  serviceRoleKey: string,
): boolean {
  if (!serviceRoleKey) return false;
  const header = req.headers.get("authorization") ?? "";
  const match = /^Bearer\s+(.+)$/i.exec(header);
  if (!match) return false;
  return timingSafeEqual(match[1].trim(), serviceRoleKey);
}

// Constant-time, so the check does not leak how many leading characters
// matched.
function timingSafeEqual(a: string, b: string): boolean {
  if (a.length !== b.length) return false;
  let diff = 0;
  for (let i = 0; i < a.length; i++) {
    diff |= a.charCodeAt(i) ^ b.charCodeAt(i);
  }
  return diff === 0;
}
