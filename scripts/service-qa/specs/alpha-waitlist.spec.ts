/**
 * Alpha waitlist (031_alpha_waitlist.sql + functions/alpha-unsubscribe).
 *
 * A signed-in user joins with the same upsert the dashboard's Alpha tab
 * sends, follows the unsubscribe link from an email (GET, then the RFC 8058
 * one-click POST after rejoining), and rejoins. The link must stop sends,
 * keep the account, be idempotent, and look the same for unknown tokens.
 */
import { randomUUID } from "node:crypto";
import { afterEach, expect, it } from "vitest";
import type { SupabaseClient } from "@supabase/supabase-js";
import { getFunctionsBaseUrl, getServiceClient } from "../clients";
import {
  createTestUser,
  deleteTestUser,
  signInWithEmailPassword,
} from "../seed";
import { recordOutcome } from "../record";

// Must match ALPHA_FORM_VERSION in web/src/pages/dashboard.tsx.
const FORM_VERSION = "2026-10-alpha-v1";
const SOURCE_PAGE = "/alpha";

const usersToDelete: string[] = [];

afterEach(async () => {
  const admin = getServiceClient();
  while (usersToDelete.length > 0) {
    try {
      await deleteTestUser(admin, usersToDelete.pop()!);
    } catch {
      // already deleted
    }
  }
});

type AdminRow = {
  consented_at: string;
  unsubscribed_at: string | null;
  unsubscribe_token: string;
  form_version: string;
  source_page: string | null;
};

async function join(client: SupabaseClient, userId: string): Promise<void> {
  const { error } = await client.from("alpha_waitlist").upsert(
    {
      user_id: userId,
      form_version: FORM_VERSION,
      source_page: SOURCE_PAGE,
      unsubscribed_at: null,
    },
    { onConflict: "user_id" },
  );
  expect(error).toBeNull();
}

async function adminRow(userId: string): Promise<AdminRow> {
  const { data, error } = await getServiceClient()
    .from("alpha_waitlist")
    .select(
      "consented_at, unsubscribed_at, unsubscribe_token, form_version, source_page",
    )
    .eq("user_id", userId)
    .single();
  expect(error).toBeNull();
  return data as AdminRow;
}

async function unsubscribe(
  token: string | null,
  method: "GET" | "POST" = "GET",
): Promise<{ status: number; contentType: string; body: string }> {
  const query = token === null ? "" : `?token=${encodeURIComponent(token)}`;
  const res = await fetch(
    `${getFunctionsBaseUrl()}/alpha-unsubscribe${query}`,
    {
      method,
      // RFC 8058: mail providers POST this body to the List-Unsubscribe URL.
      ...(method === "POST"
        ? {
            headers: { "Content-Type": "application/x-www-form-urlencoded" },
            body: "List-Unsubscribe=One-Click",
          }
        : {}),
    },
  );
  return {
    status: res.status,
    contentType: res.headers.get("content-type") ?? "",
    body: await res.text(),
  };
}

it("joins, unsubscribes by link, and rejoins without losing the account", async () => {
  const testUser = await createTestUser();
  usersToDelete.push(testUser.user.id);
  const userId = testUser.user.id;
  const { client } = await signInWithEmailPassword(
    testUser.email,
    testUser.password,
  );

  // ── Join ────────────────────────────────────────────────────────────────
  await join(client, userId);

  const { data: own, error: ownError } = await client
    .from("alpha_waitlist")
    .select("user_id, consented_at, form_version, source_page, unsubscribed_at")
    .maybeSingle();
  expect(ownError).toBeNull();
  expect(own).toMatchObject({
    user_id: userId,
    form_version: FORM_VERSION,
    source_page: SOURCE_PAGE,
    unsubscribed_at: null,
  });
  expect(Date.parse(own!.consented_at)).toBeGreaterThan(Date.now() - 60_000);

  const { error: tokenReadError } = await client
    .from("alpha_waitlist")
    .select("unsubscribe_token")
    .maybeSingle();
  expect(tokenReadError?.code).toBe("42501");

  // A second account sees nothing of the first.
  const other = await createTestUser();
  usersToDelete.push(other.user.id);
  const { client: otherClient } = await signInWithEmailPassword(
    other.email,
    other.password,
  );
  const { data: otherView, error: otherError } = await otherClient
    .from("alpha_waitlist")
    .select("user_id");
  expect(otherError).toBeNull();
  expect(otherView).toEqual([]);

  const joined = await adminRow(userId);
  expect(joined.unsubscribe_token).toMatch(/^[0-9a-f-]{36}$/);

  await recordOutcome("alpha-waitlist-01-join", {
    expectations: [
      "The dashboard's upsert creates the user's row with form_version, source_page, a fresh consented_at, and unsubscribed_at null.",
      "The signed-in user cannot read their unsubscribe_token (PostgREST 42501).",
      "A second signed-in user reads zero waitlist rows.",
    ],
    details: { own, tokenReadError, otherView },
  });

  // ── Unsubscribe by email link (GET) ─────────────────────────────────────
  const first = await unsubscribe(joined.unsubscribe_token);
  expect(first.status).toBe(200);
  expect(first.contentType).toMatch(/^text\/html/);
  expect(first.body).toMatch(/unsubscribed/i);
  expect(first.body).not.toContain(joined.unsubscribe_token);
  expect(first.body).not.toContain(testUser.email);

  const afterFirst = await adminRow(userId);
  expect(afterFirst.unsubscribed_at).not.toBeNull();

  const second = await unsubscribe(joined.unsubscribe_token);
  expect(second).toEqual(first);
  const afterSecond = await adminRow(userId);
  expect(afterSecond.unsubscribed_at).toBe(afterFirst.unsubscribed_at);

  // The account survives: password sign-in still works.
  await expect(
    signInWithEmailPassword(testUser.email, testUser.password),
  ).resolves.toBeTruthy();

  await recordOutcome("alpha-waitlist-02-unsubscribe-link", {
    expectations: [
      "GET alpha-unsubscribe?token=<token> returns HTTP 200 text/html and sets unsubscribed_at.",
      "Repeating the request returns the same page and leaves unsubscribed_at unchanged (idempotent).",
      "The account is kept: password sign-in still succeeds after unsubscribing.",
    ],
    details: {
      status: first.status,
      contentType: first.contentType,
      unsubscribedAt: afterFirst.unsubscribed_at,
      unsubscribedAtAfterRepeat: afterSecond.unsubscribed_at,
    },
  });

  // ── Unknown, malformed, and missing tokens look the same ────────────────
  const unknown = await unsubscribe(randomUUID());
  const malformed = await unsubscribe("not-a-token");
  const missing = await unsubscribe(null);
  for (const res of [unknown, malformed, missing]) {
    expect(res).toEqual(first);
  }

  await recordOutcome("alpha-waitlist-03-no-enumeration", {
    expectations: [
      "An unknown token, a malformed token, and no token each return the same status, content type, and body as a real unsubscribe.",
    ],
    details: {
      unknown: unknown.status,
      malformed: malformed.status,
      missing: missing.status,
    },
  });

  // ── Rejoin, then the RFC 8058 one-click POST ────────────────────────────
  await join(client, userId);
  const rejoined = await adminRow(userId);
  expect(rejoined.unsubscribed_at).toBeNull();
  expect(Date.parse(rejoined.consented_at)).toBeGreaterThan(
    Date.parse(joined.consented_at),
  );
  expect(rejoined.unsubscribe_token).toBe(joined.unsubscribe_token);

  const oneClick = await unsubscribe(joined.unsubscribe_token, "POST");
  expect(oneClick.status).toBe(200);
  const afterOneClick = await adminRow(userId);
  expect(afterOneClick.unsubscribed_at).not.toBeNull();

  await recordOutcome("alpha-waitlist-04-rejoin", {
    expectations: [
      "Rejoining clears unsubscribed_at and records a later consented_at than the first join.",
      "The unsubscribe token is unchanged by rejoining, so links in earlier emails still work.",
      "The RFC 8058 one-click POST to the same URL unsubscribes again.",
    ],
    details: {
      firstConsent: joined.consented_at,
      rejoinConsent: rejoined.consented_at,
      oneClickStatus: oneClick.status,
      unsubscribedAt: afterOneClick.unsubscribed_at,
    },
  });
}, 90_000);
