// GitHub App user access tokens: a token that acts as the user, limited to
// the App's permissions. With "Expire user authorization tokens" on (the
// default), GitHub returns an 8-hour access token and a 6-month refresh
// token, and rotates the refresh token on every refresh.

export type UserGrant = {
  accessToken: string;
  /** Null when the App does not expire user tokens. */
  refreshToken: string | null;
  expiresAt: string | null;
  refreshExpiresAt: string | null;
};

function expiry(seconds: unknown, nowMs: number): string | null {
  return typeof seconds === "number" ? new Date(nowMs + seconds * 1000).toISOString() : null;
}

/**
 * Reads GitHub's answer to POST /login/oauth/access_token. GitHub reports a
 * refused code or refresh token with 200 and an `error` field.
 */
export function parseUserGrant(answer: Record<string, unknown>, nowMs: number): UserGrant {
  if (typeof answer.access_token !== "string" || !answer.access_token) {
    const reason = typeof answer.error === "string" ? answer.error : "no access_token";
    throw new Error(`GitHub token exchange refused: ${reason}`);
  }
  return {
    accessToken: answer.access_token,
    refreshToken: typeof answer.refresh_token === "string" ? answer.refresh_token : null,
    expiresAt: expiry(answer.expires_in, nowMs),
    refreshExpiresAt: expiry(answer.refresh_token_expires_in, nowMs),
  };
}
