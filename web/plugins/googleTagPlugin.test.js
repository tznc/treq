// @ts-check
const vm = require('vm');
const {buildGoogleTagHeadTags, CONSENT_REGIONS} = require('./googleTagPlugin');

/**
 * Runs the inline head script the way a browser would and returns every
 * gtag() call it queued, e.g. ['consent', 'default', {...}].
 * @returns {any[][]}
 */
function gtagCalls() {
  const script = buildGoogleTagHeadTags('G-TEST').find((t) => t.innerHTML)?.innerHTML ?? '';
  /** @type {any} */
  const page = {localStorage: {getItem: () => null}};
  page.window = page;
  vm.runInNewContext(script, page);
  return page.dataLayer.map((/** @type {IArguments} */ args) => Array.from(args));
}

const consentDefaults = () =>
  gtagCalls()
    .filter(([command, action]) => command === 'consent' && action === 'default')
    .map(([, , settings]) => settings);

test('consent defaults are set before gtag config', () => {
  const commands = gtagCalls().map(([command, action]) => `${command} ${action}`);
  expect(commands.indexOf('consent default')).toBeLessThan(commands.indexOf('config G-TEST'));
});

test('analytics is denied by default in consent regions and granted elsewhere', () => {
  const [regional, everywhere] = consentDefaults();
  expect(regional).toMatchObject({region: CONSENT_REGIONS, analytics_storage: 'denied', ad_storage: 'denied'});
  expect(everywhere).toMatchObject({analytics_storage: 'granted', ad_storage: 'denied'});
  expect(everywhere.region).toBeUndefined();
});

test('covers EEA, UK and Switzerland', () => {
  expect(CONSENT_REGIONS).toEqual(expect.arrayContaining(['DE', 'FR', 'IE', 'NO', 'IS', 'LI', 'GB', 'CH']));
  expect(CONSENT_REGIONS).toHaveLength(32);
});

test('loads gtag.js for the tracking id', () => {
  const src = buildGoogleTagHeadTags('G-TEST').find((t) => t.attributes?.src)?.attributes?.src;
  expect(src).toBe('https://www.googletagmanager.com/gtag/js?id=G-TEST');
});
