/**
 * GTM metrics endpoint (024_gtm_metrics.sql + functions/gtm-metrics).
 *
 * biz-tools calls GET gtm-metrics?from=&to= with the x-gtm-metrics-secret
 * header and gets aggregate counts only. The spec seeds accounts and GitHub
 * App installations (and alpha waitlist rows when 031_alpha_waitlist.sql is
 * present) and checks the counts move by exactly that much. The local
 * database may hold rows from other specs, so it compares before and after.
 */
import { afterEach, expect, it } from "vitest";
import { getFunctionsBaseUrl, getServiceClient } from "../clients";
import {
  createTestUser,
  deleteTestUser,
  linkGithubRepo,
  signInWithEmailPassword,
} from "../seed";
import { recordOutcome } from "../record";

/** Must match GTM_METRICS_SECRET in supabase/functions/.env (written by up.sh). */
const SECRET = "service-qa-local-gtm-metrics-secret";

const COUNT_KEYS = [
  "accounts_created",
  "accounts_total",
  "github_app_installations_linked",
  "installations_total",
  "alpha_waitlist_joined",
  "alpha_waitlist_unsubscribed",
  "alpha_waitlist_active",
] as const;

type Metrics = { from: string; to: string } & Record<
  (typeof COUNT_KEYS)[number],
  number | null
>;

const usersToDelete: string[] = [];
const installationsToDelete: number[] = [];

afterEach(async () => {
  const admin = getServiceClient();
  if (installationsToDelete.length > 0) {
    await admin
      .from("github_app_installations")
      .delete()
      .in("id", installationsToDelete.splice(0));
  }
  while (usersToDelete.length > 0) {
    try {
      await deleteTestUser(admin, usersToDelete.pop()!);
    } catch {
      // already deleted
    }
  }
});

function day(offset: number): string {
  return new Date(Date.now() + offset * 86_400_000).toISOString().slice(0, 10);
}

// Yesterday to tomorrow (UTC), so rows created now are inside the window
// even when the run crosses midnight.
const FROM = day(-1);
const TO = day(1);

async function getMetrics(
  headers: Record<string, string>,
  query = `?from=${FROM}&to=${TO}`,
): Promise<{ status: number; text: string }> {
  const res = await fetch(`${getFunctionsBaseUrl()}/gtm-metrics${query}`, {
    headers,
  });
  return { status: res.status, text: await res.text() };
}

async function metrics(): Promise<Metrics> {
  const res = await getMetrics({ "x-gtm-metrics-secret": SECRET });
  expect(res.status).toBe(200);
  return JSON.parse(res.text) as Metrics;
}

async function hasAlphaWaitlist(): Promise<boolean> {
  const { error } = await getServiceClient()
    .from("alpha_waitlist")
    .select("user_id")
    .limit(1);
  return error === null;
}

it("rejects a missing or wrong secret and bad dates", async () => {
  const missing = await getMetrics({});
  const wrong = await getMetrics({ "x-gtm-metrics-secret": `${SECRET}x` });
  const prefix = await getMetrics({
    "x-gtm-metrics-secret": SECRET.slice(0, -1),
  });
  for (const res of [missing, wrong, prefix]) {
    expect(res.status).toBe(401);
    expect(res.text).not.toMatch(/accounts_/);
  }

  const reversed = await getMetrics(
    { "x-gtm-metrics-secret": SECRET },
    `?from=${TO}&to=${FROM}`,
  );
  const malformed = await getMetrics(
    { "x-gtm-metrics-secret": SECRET },
    "?from=2026-02-30&to=2026-03-01",
  );
  expect(reversed.status).toBe(400);
  expect(malformed.status).toBe(400);

  await recordOutcome("gtm-metrics-01-auth", {
    expectations: [
      "No x-gtm-metrics-secret header, a wrong secret, and a prefix of the secret each return HTTP 401 with no counts.",
      "With the right secret, from after to and an impossible date each return HTTP 400.",
    ],
    details: {
      missing: missing.status,
      wrong: wrong.status,
      prefix: prefix.status,
      reversed: reversed.status,
      malformed: malformed.status,
    },
  });
}, 60_000);

it("returns aggregate counts that match seeded data and nothing else", async () => {
  const admin = getServiceClient();
  const alpha = await hasAlphaWaitlist();
  const before = await metrics();

  // Two new accounts: one links two installations, one links none. A third
  // installation is unlinked.
  const linked = await createTestUser();
  const plain = await createTestUser();
  usersToDelete.push(linked.user.id, plain.user.id);
  const repo = await linkGithubRepo(admin, linked.user.id);
  installationsToDelete.push(repo.installationId);
  const second = await linkGithubRepo(admin, linked.user.id);
  installationsToDelete.push(second.installationId);
  const unlinkedId = Math.floor(Math.random() * 1_000_000_000) + 3_000_000;
  const { error: unlinkedError } = await admin
    .from("github_app_installations")
    .insert({
      id: unlinkedId,
      account_login: "service-qa-unlinked",
      account_type: "Organization",
      app_id: 1,
      linked_user_id: null,
    });
  expect(unlinkedError).toBeNull();
  installationsToDelete.push(unlinkedId);

  // With the waitlist table: both accounts join, one leaves.
  if (alpha) {
    for (const user of [linked, plain]) {
      const { client } = await signInWithEmailPassword(
        user.email,
        user.password,
      );
      const { error } = await client.from("alpha_waitlist").upsert(
        {
          user_id: user.user.id,
          form_version: "service-qa",
          source_page: "/dashboard",
          unsubscribed_at: null,
        },
        { onConflict: "user_id" },
      );
      expect(error).toBeNull();
    }
    const { error } = await admin
      .from("alpha_waitlist")
      .update({ unsubscribed_at: new Date().toISOString() })
      .eq("user_id", plain.user.id);
    expect(error).toBeNull();
  }

  const after = await metrics();
  const delta = (key: (typeof COUNT_KEYS)[number]) =>
    (after[key] ?? 0) - (before[key] ?? 0);

  expect(after.from).toBe(FROM);
  expect(after.to).toBe(TO);
  expect(delta("accounts_created")).toBe(2);
  expect(delta("accounts_total")).toBe(2);
  expect(delta("installations_total")).toBe(3);
  expect(delta("github_app_installations_linked")).toBe(1);
  if (alpha) {
    expect(delta("alpha_waitlist_joined")).toBe(2);
    expect(delta("alpha_waitlist_unsubscribed")).toBe(1);
    expect(delta("alpha_waitlist_active")).toBe(1);
  } else {
    expect(after.alpha_waitlist_joined).toBeNull();
    expect(after.alpha_waitlist_unsubscribed).toBeNull();
    expect(after.alpha_waitlist_active).toBeNull();
  }

  // Only the window and counts: no emails, user IDs, or installation IDs.
  expect(Object.keys(after).sort()).toEqual(
    ["from", "to", ...COUNT_KEYS].sort(),
  );
  for (const key of COUNT_KEYS) {
    expect(
      after[key] === null || Number.isInteger(after[key]),
      `${key} is a count`,
    ).toBe(true);
  }
  const body = JSON.stringify(after);
  for (const secretish of [
    linked.email,
    plain.email,
    linked.user.id,
    plain.user.id,
    String(repo.installationId),
    String(unlinkedId),
    "@",
  ]) {
    expect(body).not.toContain(secretish);
  }

  await recordOutcome("gtm-metrics-02-counts", {
    expectations: [
      "After seeding 2 accounts and 3 installations (2 linked to one account), accounts_created and accounts_total rise by 2, installations_total by 3, and github_app_installations_linked by 1.",
      "Waitlist counts rise by 2 joined, 1 unsubscribed, 1 active when 031_alpha_waitlist.sql is applied, and are null when it is not.",
      "The response has only from, to, and the count keys; it contains no email, user ID, or installation ID.",
    ],
    details: { alphaTable: alpha, before, after },
  });
}, 90_000);
