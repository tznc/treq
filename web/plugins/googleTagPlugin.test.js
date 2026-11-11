// @ts-check
const {buildGoogleTagHeadTags, CONSENT_REGIONS} = require('./googleTagPlugin');

/** @param {any[]} tags */
const inline = (tags) => tags.filter((/** @type {any} */ t) => t.innerHTML).map((/** @type {any} */ t) => t.innerHTML).join('\n');

test('consent defaults are set before gtag config', () => {
  const html = inline(buildGoogleTagHeadTags('G-TEST'));
  const deniedDefault = html.indexOf("gtag('consent', 'default'");
  const config = html.indexOf("gtag('config', 'G-TEST')");
  expect(deniedDefault >= 0 && config > deniedDefault).toBeTruthy();
});

test('analytics is denied by default in consent regions and granted elsewhere', () => {
  const html = inline(buildGoogleTagHeadTags('G-TEST'));
  const defaults = [...html.matchAll(/gtag\('consent', 'default', (\{.*?\})\);/g)].map((m) => JSON.parse(m[1]));
  expect(defaults[0].region).toEqual(CONSENT_REGIONS);
  expect(defaults[0].analytics_storage).toBe('denied');
  expect(defaults[1].region).toBe(undefined);
  expect(defaults[1].analytics_storage).toBe('granted');
  for (const d of defaults) expect(d.ad_storage).toBe('denied');
});

test('a stored choice is restored before config', () => {
  const html = inline(buildGoogleTagHeadTags('G-TEST'));
  expect(html.indexOf("gtag('consent', 'update'") < html.indexOf("gtag('config'")).toBeTruthy();
});

test('covers EEA, UK and Switzerland', () => {
  for (const code of ['DE', 'FR', 'IE', 'NO', 'IS', 'LI', 'GB', 'CH']) expect(CONSENT_REGIONS.includes(code)).toBeTruthy();
  expect(CONSENT_REGIONS.length).toBe(32);
});

test('loads gtag.js for the tracking id', () => {
  const src = buildGoogleTagHeadTags('G-TEST').find((t) => t.attributes?.src)?.attributes?.src;
  expect(src).toBe('https://www.googletagmanager.com/gtag/js?id=G-TEST');
});
