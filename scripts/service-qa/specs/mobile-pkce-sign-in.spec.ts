/**
 * The mobile app's in-app sign-in (`core::auth`): GoTrue PKCE straight from
 * Rust, with `treq://auth/callback` as the redirect.
 *
 * OAuth provider secrets are not available locally, so the code is minted by
 * a PKCE magic link (same flow state and `grant_type=pkce` exchange the OAuth
 * path uses). The authorize step is checked against GitHub.
 */
import { createHash, randomBytes } from "node:crypto";
import { it, expect, afterEach } from "vitest";
import { getAnonKey, getServiceClient, resolveLocalKeys } from "../clients";
import { createTestUser, deleteTestUser } from "../seed";
import { recordOutcome } from "../record";

const REDIRECT = "treq://auth/callback";
const MAILPIT = "http://127.0.0.1:54324";
const usersToDelete: string[] = [];

afterEach(async () => {
  const admin = getServiceClient();
  while (usersToDelete.length > 0) {
    await deleteTestUser(admin, usersToDelete.pop()!).catch(() => {});
  }
});

function pkce() {
  const verifier = randomBytes(32).toString("base64url");
  const challenge = createHash("sha256").update(verifier).digest("base64url");
  return { verifier, challenge };
}

function headers() {
  return { "Content-Type": "application/json", apikey: getAnonKey() };
}

/** What `Auth::exchange_code` sends. */
function exchange(code: string, verifier: string) {
  return fetch(`${resolveLocalKeys().url}/auth/v1/token?grant_type=pkce`, {
    method: "POST",
    headers: { ...headers(), Authorization: `Bearer ${getAnonKey()}` },
    body: JSON.stringify({ auth_code: code, code_verifier: verifier }),
  });
}

async function magicLinkFor(email: string): Promise<string> {
  for (let i = 0; i < 40; i++) {
    const list = await (
      await fetch(`${MAILPIT}/api/v1/search?query=${encodeURIComponent(`to:"${email}"`)}`)
    ).json();
    const id = list.messages?.[0]?.ID;
    if (id) {
      const msg = await (await fetch(`${MAILPIT}/api/v1/message/${id}`)).json();
      const match = /href="([^"]*\/auth\/v1\/verify[^"]*)"/.exec(msg.HTML ?? "");
      if (match) return match[1].replaceAll("&amp;", "&");
    }
    await new Promise((r) => setTimeout(r, 250));
  }
  throw new Error("magic link email not received");
}

// Google is the same GoTrue path; it needs its OIDC discovery document, which
// sandboxes without outbound TLS cannot fetch.
it.each(["github"])(
  "authorize with a PKCE challenge redirects to %s",
  async (provider) => {
    const { challenge } = pkce();
    const url = new URL(`${resolveLocalKeys().url}/auth/v1/authorize`);
    url.search = new URLSearchParams({
      provider,
      redirect_to: REDIRECT,
      code_challenge: challenge,
      code_challenge_method: "s256",
    }).toString();
    const res = await fetch(url, { redirect: "manual" });
    const location = res.headers.get("location") ?? "";
    expect(res.status).toBe(302);
    expect(new URL(location).searchParams.get("state")).toBeTruthy();

    await recordOutcome(`mobile-pkce-01-authorize-${provider}`, {
      expectations: [
        `GET /auth/v1/authorize?provider=${provider} with an s256 challenge answers 302 to the provider.`,
        "The redirect carries a GoTrue state, so the PKCE flow state was stored.",
      ],
      details: { status: res.status, host: new URL(location).host },
    });
  },
  30000,
);

it("trades a PKCE code for a session once, and only with its verifier", async () => {
  const { user, email } = await createTestUser();
  usersToDelete.push(user.id);
  const { verifier, challenge } = pkce();

  const otp = await fetch(
    `${resolveLocalKeys().url}/auth/v1/otp?redirect_to=${encodeURIComponent(REDIRECT)}`,
    {
      method: "POST",
      headers: headers(),
      body: JSON.stringify({
        email,
        create_user: false,
        code_challenge: challenge,
        code_challenge_method: "s256",
      }),
    },
  );
  expect(otp.status).toBe(200);

  const verify = await fetch(await magicLinkFor(email), { redirect: "manual" });
  const callback = new URL(verify.headers.get("location") ?? "");
  expect(`${callback.protocol}//${callback.host}${callback.pathname}`).toBe(REDIRECT);
  const code = callback.searchParams.get("code")!;
  expect(code).toBeTruthy();

  const wrong = await exchange(code, pkce().verifier);
  expect(wrong.status).toBeGreaterThanOrEqual(400);

  const ok = await exchange(code, verifier);
  const session = await ok.json();
  expect(ok.status).toBe(200);
  expect(session.refresh_token).toBeTruthy();
  expect(session.user.id).toBe(user.id);

  const replay = await exchange(code, verifier);
  expect(replay.status).toBeGreaterThanOrEqual(400);

  await recordOutcome("mobile-pkce-02-code-exchange", {
    expectations: [
      "GoTrue redirects to treq://auth/callback?code=… for a PKCE flow.",
      "POST /auth/v1/token?grant_type=pkce {auth_code, code_verifier} returns a session for the right user.",
      "A wrong verifier and a replayed code are both rejected.",
    ],
    details: { wrong: wrong.status, ok: ok.status, replay: replay.status },
  });
}, 60000);
