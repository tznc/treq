import test from 'node:test';
import assert from 'node:assert/strict';
import {shouldAskForConsent} from './consent.ts';

test('asks European time zones with no stored choice', () => {
  assert.equal(shouldAskForConsent('Europe/Berlin', null), true);
  assert.equal(shouldAskForConsent('Europe/London', null), true);
});

test('does not ask once a choice is stored', () => {
  assert.equal(shouldAskForConsent('Europe/Berlin', 'granted'), false);
  assert.equal(shouldAskForConsent('Europe/Berlin', 'denied'), false);
});

test('does not ask outside Europe', () => {
  assert.equal(shouldAskForConsent('Asia/Singapore', null), false);
  assert.equal(shouldAskForConsent('America/New_York', null), false);
});

test('Atlantic islands in the EEA are asked', () => {
  assert.equal(shouldAskForConsent('Atlantic/Canary', null), true);
  assert.equal(shouldAskForConsent('Atlantic/Reykjavik', null), true);
});
