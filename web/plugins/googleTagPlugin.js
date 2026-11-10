// @ts-check
// Replaces the preset's gtag plugin so Google Consent Mode defaults run before
// gtag('config'). Preset plugins inject head tags before site plugins, so the
// consent default cannot be placed ahead of the preset's own config call.

// EEA, UK and Switzerland: analytics cookies need consent before they are set.
const CONSENT_REGIONS = [
  'AT', 'BE', 'BG', 'HR', 'CY', 'CZ', 'DK', 'EE', 'FI', 'FR', 'DE', 'GR', 'HU', 'IE', 'IT', 'LV',
  'LT', 'LU', 'MT', 'NL', 'PL', 'PT', 'RO', 'SK', 'SI', 'ES', 'SE', 'IS', 'LI', 'NO', 'GB', 'CH',
];

const CONSENT_STORAGE_KEY = 'treq-analytics-consent';

const ADS_DENIED = {ad_storage: 'denied', ad_user_data: 'denied', ad_personalization: 'denied'};

/**
 * @param {string} trackingID
 * @returns {import('@docusaurus/types').HtmlTagObject[]}
 */
function buildGoogleTagHeadTags(trackingID) {
  const regionDefault = {...ADS_DENIED, analytics_storage: 'denied', region: CONSENT_REGIONS, wait_for_update: 500};
  const globalDefault = {...ADS_DENIED, analytics_storage: 'granted'};
  return [
    {tagName: 'link', attributes: {rel: 'preconnect', href: 'https://www.google-analytics.com'}},
    {tagName: 'link', attributes: {rel: 'preconnect', href: 'https://www.googletagmanager.com'}},
    {
      tagName: 'script',
      innerHTML: [
        'window.dataLayer = window.dataLayer || [];',
        'function gtag(){dataLayer.push(arguments);}',
        `gtag('consent', 'default', ${JSON.stringify(regionDefault)});`,
        `gtag('consent', 'default', ${JSON.stringify(globalDefault)});`,
        `try { var c = localStorage.getItem('${CONSENT_STORAGE_KEY}'); if (c === 'granted' || c === 'denied') gtag('consent', 'update', {analytics_storage: c}); } catch (e) {}`,
        "gtag('js', new Date());",
        `gtag('config', '${trackingID}');`,
      ].join('\n'),
    },
    {
      tagName: 'script',
      attributes: {async: true, src: `https://www.googletagmanager.com/gtag/js?id=${trackingID}`},
    },
  ];
}

/** @param {import('@docusaurus/types').LoadContext} _context @param {{trackingID: string}} options */
function googleTagPlugin(_context, options) {
  return {
    name: 'google-tag-plugin',
    // Docusaurus's own client module: sends page_view on client-side navigation.
    getClientModules() {
      return [require.resolve('@docusaurus/plugin-google-gtag/lib/gtag')];
    },
    injectHtmlTags() {
      return {headTags: buildGoogleTagHeadTags(options.trackingID)};
    },
  };
}

module.exports = googleTagPlugin;
module.exports.buildGoogleTagHeadTags = buildGoogleTagHeadTags;
module.exports.CONSENT_REGIONS = CONSENT_REGIONS;
module.exports.CONSENT_STORAGE_KEY = CONSENT_STORAGE_KEY;
