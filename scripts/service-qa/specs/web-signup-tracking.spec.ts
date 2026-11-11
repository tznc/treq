/**
 * treq.dev sign_up tracking (web/src/lib/analytics.ts, isFirstSignIn).
 *
 * The auth callback fires GA4 `sign_up` when the session's user is on their
 * first sign-in, judged from Supabase Auth's own timestamps: last_sign_in_at
 * within 60 seconds of email_confirmed_at (or created_at). This spec signs
 * real users in against local Auth and runs isFirstSignIn on the user object
 * sign-in returns, the same object the callback reads from the session.
 *
 * OAuth (Google, GitHub) is out of scope here: service-qa uses email/password
 * only. Those providers set the same columns at account creation.
 */
import type { User } from "@supabase/supabase-js";
import { afterEach, expect, it } from "@jest/globals";
import { isFirstSignIn } from "../../../web/src/lib/analytics";
import { getServiceClient } from "../clients";
import { recordOutcome } from "../record";
import {
  createTestUser,
  deleteTestUser,
  signInWithEmailPassword,
} from "../seed";

const created: string[] = [];
afterEach(async () => {
  const admin = getServiceClient();
  while (created.length) await deleteTestUser(admin, created.pop() as string);
});

const timestamps = (u: User) => ({
  created_at: u.created_at,
  email_confirmed_at: u.email_confirmed_at,
  last_sign_in_at: u.last_sign_in_at,
  provider: u.app_metadata.provider,
});

// Past the 60 second window isFirstSignIn allows for a first session.
const pastWindow = () => new Promise((resolve) => setTimeout(resolve, 61_000));

it("counts a first sign-in once, ignores a later one, and measures from email confirmation", async () => {
  const admin = getServiceClient();

  // Fresh signup: local Auth confirms immediately (enable_confirmations = false).
  const fresh = await createTestUser();
  created.push(fresh.user.id);
  const first = await signInWithEmailPassword(fresh.email, fresh.password);

  // Confirm-later account: created unconfirmed now, confirmed after the wait.
  const late = `late-${Date.now()}@service-qa.test`;
  const latePassword = `pw-${Date.now()}-Aa1!`;
  const { data: lateCreate, error: lateErr } =
    await admin.auth.admin.createUser({
      email: late,
      password: latePassword,
      email_confirm: false,
    });
  expect(lateErr).toBeNull();
  created.push(lateCreate.user!.id);

  await pastWindow();

  const again = await signInWithEmailPassword(fresh.email, fresh.password);

  const { error: confirmErr } = await admin.auth.admin.updateUserById(
    lateCreate.user!.id,
    {
      email_confirm: true,
    },
  );
  expect(confirmErr).toBeNull();
  const lateSignIn = await signInWithEmailPassword(late, latePassword);

  expect(first.user.app_metadata.provider).toBe("email");
  expect(isFirstSignIn(first.user)).toBe(true);
  expect(isFirstSignIn(again.user)).toBe(false);
  expect(
    Date.parse(lateSignIn.user.email_confirmed_at!) -
      Date.parse(lateSignIn.user.created_at),
  ).toBeGreaterThan(60_000);
  expect(isFirstSignIn(lateSignIn.user)).toBe(true);

  await recordOutcome("web-signup-tracking-01-first-sign-in", {
    expectations: [
      "A fresh email signup's first sign-in is a first sign-in (sign_up fires); provider is 'email'.",
      "The same user signing in again more than 60 seconds later is not a first sign-in.",
    ],
    details: { first: timestamps(first.user), again: timestamps(again.user) },
  });
  await recordOutcome("web-signup-tracking-02-confirmed-later", {
    expectations: [
      "An account confirmed more than 60 seconds after creation still counts on its first sign-in, because the window starts at email_confirmed_at.",
    ],
    details: timestamps(lateSignIn.user),
  });
}, 120_000);
