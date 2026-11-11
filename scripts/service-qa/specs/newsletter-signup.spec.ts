/**
 * treq.dev "Get new guides by email" (079_newsletter_subscribers.sql).
 *
 * The form calls subscribe_newsletter() with the anon key. The spec calls it
 * the same way and checks the row as service_role: trimmed and lowercased,
 * the first sign-up page kept, repeats silently ignored, bad addresses
 * rejected, and no API role able to read the list.
 */
import { afterAll, expect, it } from "@jest/globals";
import { getAnonClient, getServiceClient } from "../clients";
import { recordOutcome } from "../record";
import {
  createTestUser,
  deleteTestUser,
  signInWithEmailPassword,
} from "../seed";

const email = `reader-${Date.now()}@service-qa.test`;
const cleanup: Array<() => Promise<unknown>> = [];

afterAll(async () => {
  const admin = getServiceClient();
  await admin.from("newsletter_subscribers").delete().eq("email", email);
  for (const step of cleanup) await step();
});

it("stores one normalized sign-up per address and keeps the list private", async () => {
  const admin = getServiceClient();
  const anon = getAnonClient();

  const first = await anon.rpc("subscribe_newsletter", {
    p_email: `  ${email.toUpperCase()} `,
    p_source_path: "/learn/stacked-prs",
  });
  const repeat = await anon.rpc("subscribe_newsletter", {
    p_email: email,
    p_source_path: "/tools/branch-visualizer",
  });
  const invalid = await anon.rpc("subscribe_newsletter", {
    p_email: "not-an-email",
    p_source_path: "/learn",
  });

  const { data: rows } = await admin
    .from("newsletter_subscribers")
    .select("email, source_path, consented_at, unsubscribed_at")
    .eq("email", email);

  expect(first.error).toBeNull();
  expect(repeat.error).toBeNull();
  expect(invalid.error?.code).toBe("22023");
  expect(rows).toHaveLength(1);
  expect(rows?.[0]).toMatchObject({
    email,
    source_path: "/learn/stacked-prs",
    unsubscribed_at: null,
  });

  await recordOutcome("newsletter-signup-01-subscribe", {
    expectations: [
      "An anon sign-up stores one row, trimmed and lowercased, with the page it came from.",
      "A repeat sign-up returns no error and changes nothing, so the RPC does not reveal who is subscribed.",
      "An invalid address is rejected with SQLSTATE 22023.",
    ],
    details: { row: rows?.[0], invalidError: invalid.error },
  });

  const u = await createTestUser();
  cleanup.push(() => deleteTestUser(admin, u.user.id));
  const { client: signedIn } = await signInWithEmailPassword(
    u.email,
    u.password,
  );
  const anonRead = await anon.from("newsletter_subscribers").select("email");
  const userRead = await signedIn
    .from("newsletter_subscribers")
    .select("email");
  const anonInsert = await anon
    .from("newsletter_subscribers")
    .insert({ email: `direct-${email}` });

  expect(anonRead.error?.code).toBe("42501");
  expect(userRead.error?.code).toBe("42501");
  expect(anonInsert.error?.code).toBe("42501");

  await recordOutcome("newsletter-signup-02-private", {
    expectations: [
      "Anonymous and signed-in API users cannot read the subscriber list (permission denied, 42501).",
      "Anonymous users cannot insert into the table directly; the RPC is the only way in.",
    ],
    details: {
      anonRead: anonRead.error?.code,
      userRead: userRead.error?.code,
      anonInsert: anonInsert.error?.code,
    },
  });
});
