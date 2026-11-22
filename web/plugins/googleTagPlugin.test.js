// @ts-check
const vm = require('vm');
const {buildGoogleTagHeadTags, CONSENT_REGIONS} = require('./googleTagPlugin');

/**
 * Runs the inline head script the way a browser would and returns its window.
 * @returns {any}
 */
function runHeadScript(navigator = {userAgent: 'Mozilla/5.0 Chrome/131.0', webdriver: false}) {
  const script = buildGoogleTagHeadTags('G-TEST').find((t) => t.innerHTML)?.innerHTML ?? '';
  /** @type {any} */
  const page = {localStorage: {getItem: () => null}, navigator};
  page.window = page;
  vm.runInNewContext(script, page);
  return page;
}

/**
 * Every gtag() call the head script queued, e.g. ['consent', 'default', {...}].
 * @returns {any[][]}
 */
function gtagCalls() {
  return runHeadScript().dataLayer.map((/** @type {IArguments} */ args) => Array.from(args));
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

test('keeps analytics on for real browsers', () => {
  expect(runHeadScript()['ga-disable-G-TEST']).toBeUndefined();
});

test.each([
  ['automated browsers', {userAgent: 'Mozilla/5.0 Chrome/131.0', webdriver: true}],
  ['headless Chrome', {userAgent: 'Mozilla/5.0 HeadlessChrome/131.0', webdriver: false}],
  ['crawlers', {userAgent: 'Mozilla/5.0 (compatible; AhrefsBot/7.0)', webdriver: false}],
  ['Lighthouse', {userAgent: 'Mozilla/5.0 Chrome-Lighthouse', webdriver: false}],
])('disables analytics for %s', (_name, navigator) => {
  expect(runHeadScript(navigator)['ga-disable-G-TEST']).toBe(true);
});
