import {shouldAskForConsent} from './consent';

test('asks European time zones with no stored choice', () => {
  expect(shouldAskForConsent('Europe/Berlin', null)).toBe(true);
  expect(shouldAskForConsent('Europe/London', null)).toBe(true);
});

test('does not ask once a choice is stored', () => {
  expect(shouldAskForConsent('Europe/Berlin', 'granted')).toBe(false);
  expect(shouldAskForConsent('Europe/Berlin', 'denied')).toBe(false);
});

test('does not ask outside Europe', () => {
  expect(shouldAskForConsent('Asia/Singapore', null)).toBe(false);
  expect(shouldAskForConsent('America/New_York', null)).toBe(false);
});

test('Atlantic islands in the EEA are asked', () => {
  expect(shouldAskForConsent('Atlantic/Canary', null)).toBe(true);
  expect(shouldAskForConsent('Atlantic/Reykjavik', null)).toBe(true);
});
