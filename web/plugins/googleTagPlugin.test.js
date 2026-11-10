// @ts-check
const test = require('node:test');
const assert = require('node:assert/strict');
const {buildGoogleTagHeadTags, CONSENT_REGIONS} = require('./googleTagPlugin');

/** @param {any[]} tags */
const inline = (tags) => tags.filter((/** @type {any} */ t) => t.innerHTML).map((/** @type {any} */ t) => t.innerHTML).join('\n');

test('consent defaults are set before gtag config', () => {
  const html = inline(buildGoogleTagHeadTags('G-TEST'));
  const deniedDefault = html.indexOf("gtag('consent', 'default'");
  const config = html.indexOf("gtag('config', 'G-TEST')");
  assert.ok(deniedDefault >= 0 && config > deniedDefault);
});

test('analytics is denied by default in consent regions and granted elsewhere', () => {
  const html = inline(buildGoogleTagHeadTags('G-TEST'));
  const defaults = [...html.matchAll(/gtag\('consent', 'default', (\{.*?\})\);/g)].map((m) => JSON.parse(m[1]));
  assert.deepEqual(defaults[0].region, CONSENT_REGIONS);
  assert.equal(defaults[0].analytics_storage, 'denied');
  assert.equal(defaults[1].region, undefined);
  assert.equal(defaults[1].analytics_storage, 'granted');
  for (const d of defaults) assert.equal(d.ad_storage, 'denied');
});

test('a stored choice is restored before config', () => {
  const html = inline(buildGoogleTagHeadTags('G-TEST'));
  assert.ok(html.indexOf("gtag('consent', 'update'") < html.indexOf("gtag('config'"));
});

test('covers EEA, UK and Switzerland', () => {
  for (const code of ['DE', 'FR', 'IE', 'NO', 'IS', 'LI', 'GB', 'CH']) assert.ok(CONSENT_REGIONS.includes(code));
  assert.equal(CONSENT_REGIONS.length, 32);
});

test('loads gtag.js for the tracking id', () => {
  const src = buildGoogleTagHeadTags('G-TEST').find((t) => t.attributes?.src)?.attributes?.src;
  assert.equal(src, 'https://www.googletagmanager.com/gtag/js?id=G-TEST');
});
