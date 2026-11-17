/**
 * Worked example: desktop auth handoff that useAuth.exchangeToken depends on.
 *
 * Uses Auth email/password signup+login (service-qa overrides emailSignup on).
 * Web calls create_desktop_token (RPC), then the desktop hits
 * functions/v1/exchange-desktop-token and receives a session. Tokens are
 * single-use and reject when missing/invalid.
 *
 * Each case signs up / signs in / tears down the Auth user.
 */
import { it, expect, afterEach } from "@jest/globals";
import {
  getAnonClient,
  getAnonKey,
  getFunctionsBaseUrl,
  getServiceClient,
} from "../clients";
import { FEATURES } from "../features";
import {
  createDesktopToken,
  createTestUser,
  deleteTestUser,
  signInWithEmailPassword,
  type TestUser,
} from "../seed";
import { recordOutcome } from "../record";

const usersToDelete: string[] = [];

afterEach(async () => {
  const admin = getServiceClient();
  while (usersToDelete.length > 0) {
    const id = usersToDelete.pop()!;
    try {
      await deleteTestUser(admin, id);
    } catch {
      // already deleted
    }
  }
});

async function exchangeDesktopToken(token: string): Promise<Response> {
  return fetch(`${getFunctionsBaseUrl()}/exchange-desktop-token`, {
    method: "POST",
    headers: {
      "Content-Type": "application/json",
      Authorization: `Bearer ${getAnonKey()}`,
      apikey: getAnonKey(),
    },
    body: JSON.stringify({ token }),
  });
}

async function signedInUser(): Promise<{
  testUser: TestUser;
  client: ReturnType<typeof getAnonClient>;
}> {
  expect(FEATURES.emailSignup).toBe(true);
  const testUser = await createTestUser();
  usersToDelete.push(testUser.user.id);
  const { client } = await signInWithEmailPassword(
    testUser.email,
    testUser.password,
  );
  return { testUser, client };
}

it("exchanges a one-time desktop token for a session", async () => {
  const { testUser, client } = await signedInUser();

  const token = await createDesktopToken(client);
  const response = await exchangeDesktopToken(token);
  expect(response.status).toBe(200);

  const body = (await response.json()) as {
    access_token?: string;
    refresh_token?: string;
    error?: string;
  };
  expect(body.error).toBeUndefined();
  expect(typeof body.access_token).toBe("string");
  expect(typeof body.refresh_token).toBe("string");

  const sessionClient = getAnonClient();
  const { data: sessionData, error: sessionError } =
    await sessionClient.auth.setSession({
      access_token: body.access_token!,
      refresh_token: body.refresh_token!,
    });
  expect(sessionError).toBeNull();
  expect(sessionData.session?.user.email).toBe(testUser.email);

  await client.auth.signOut();
  await sessionClient.auth.signOut();

  await recordOutcome("desktop-token-exchange-01-happy-path", {
    expectations: [
      "exchange-desktop-token returns HTTP 200 with access_token and refresh_token.",
      "setSession with those tokens yields a session for the same user email.",
    ],
    details: {
      email: testUser.email,
      httpStatus: response.status,
      userId: sessionData.session?.user.id,
    },
  });
}, 60_000);

it("rejects a reused desktop token", async () => {
  const { client } = await signedInUser();

  const token = await createDesktopToken(client);
  const first = await exchangeDesktopToken(token);
  expect(first.status).toBe(200);

  const second = await exchangeDesktopToken(token);
  expect(second.status).toBe(401);
  const body = (await second.json()) as { error?: string };
  expect(body.error).toMatch(/invalid|expired/i);

  await client.auth.signOut();

  await recordOutcome("desktop-token-exchange-02-single-use", {
    expectations: [
      "A second exchange of the same token returns HTTP 401.",
      "The error body reports the token as invalid or expired.",
    ],
    details: { httpStatus: second.status, error: body.error },
  });
}, 60_000);

it("rejects a missing token body", async () => {
  const response = await fetch(
    `${getFunctionsBaseUrl()}/exchange-desktop-token`,
    {
      method: "POST",
      headers: {
        "Content-Type": "application/json",
        Authorization: `Bearer ${getAnonKey()}`,
        apikey: getAnonKey(),
      },
      body: JSON.stringify({}),
    },
  );
  expect(response.status).toBe(400);
  const body = (await response.json()) as { error?: string };
  expect(body.error).toMatch(/token/i);

  await recordOutcome("desktop-token-exchange-03-missing-token", {
    expectations: [
      "POST without a token field returns HTTP 400.",
      "The error body mentions that a token is required.",
    ],
    details: { httpStatus: response.status, error: body.error },
  });
}, 60_000);
