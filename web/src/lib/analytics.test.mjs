import test from 'node:test';
import assert from 'node:assert/strict';
import {isFirstSignIn, trackEvent} from './analytics.ts';

const user = (created, confirmed, lastSignIn) => ({
  created_at: created,
  email_confirmed_at: confirmed,
  last_sign_in_at: lastSignIn,
});

test('OAuth signup: created and signed in together is a first sign-in', () => {
  assert.equal(isFirstSignIn(user('2026-11-10T10:00:00Z', '2026-11-10T10:00:00Z', '2026-11-10T10:00:01Z')), true);
});

test('email signup: first sign-in happens at confirmation, not creation', () => {
  assert.equal(isFirstSignIn(user('2026-11-10T09:00:00Z', '2026-11-10T10:00:00Z', '2026-11-10T10:00:02Z')), true);
});

test('returning user is not a first sign-in', () => {
  assert.equal(isFirstSignIn(user('2026-10-01T00:00:00Z', '2026-10-01T00:00:00Z', '2026-11-10T10:00:00Z')), false);
});

test('missing timestamps are not a first sign-in', () => {
  assert.equal(isFirstSignIn(user('2026-11-10T10:00:00Z', null, null)), false);
});

test('trackEvent forwards to gtag and is a no-op without it', () => {
  const calls = [];
  globalThis.window = {gtag: (...args) => calls.push(args)};
  trackEvent('sign_up', {method: 'github'});
  assert.deepEqual(calls, [['event', 'sign_up', {method: 'github'}]]);
  globalThis.window = {};
  assert.doesNotThrow(() => trackEvent('sign_up', {method: 'github'}));
});
