import {isFirstSignIn, trackEvent} from './analytics';

const user = (created: string, confirmed: string | null, lastSignIn: string | null) => ({
  created_at: created,
  email_confirmed_at: confirmed,
  last_sign_in_at: lastSignIn,
});

test('OAuth signup: created and signed in together is a first sign-in', () => {
  expect(isFirstSignIn(user('2026-11-10T10:00:00Z', '2026-11-10T10:00:00Z', '2026-11-10T10:00:01Z'))).toBe(true);
});

test('email signup: first sign-in happens at confirmation, not creation', () => {
  expect(isFirstSignIn(user('2026-11-10T09:00:00Z', '2026-11-10T10:00:00Z', '2026-11-10T10:00:02Z'))).toBe(true);
});

test('returning user is not a first sign-in', () => {
  expect(isFirstSignIn(user('2026-10-01T00:00:00Z', '2026-10-01T00:00:00Z', '2026-11-10T10:00:00Z'))).toBe(false);
});

test('missing timestamps are not a first sign-in', () => {
  expect(isFirstSignIn(user('2026-11-10T10:00:00Z', null, null))).toBe(false);
});

test('trackEvent forwards to gtag and is a no-op without it', () => {
  const calls: unknown[][] = [];
  const g = globalThis as unknown as {window: {gtag?: (...args: unknown[]) => void}};
  g.window = {
    gtag: (...args) => {
      calls.push(args);
    },
  };
  trackEvent('sign_up', {method: 'github'});
  expect(calls).toEqual([['event', 'sign_up', {method: 'github'}]]);
  g.window = {};
  expect(() => trackEvent('sign_up', {method: 'github'})).not.toThrow();
});
