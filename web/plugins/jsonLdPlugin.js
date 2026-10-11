'use strict';

const SITE_URL = 'https://treq.dev';

module.exports = function jsonLdPlugin() {
  return {
    name: 'json-ld-plugin',
    injectHtmlTags() {
      return {
        headTags: [
          {
            tagName: 'script',
            attributes: {type: 'application/ld+json'},
            innerHTML: JSON.stringify({
              '@context': 'https://schema.org',
              '@type': 'WebSite',
              name: 'Treq',
              url: SITE_URL,
              description: 'The Open Source Graphite Alternative',
              potentialAction: {
                '@type': 'SearchAction',
                target: {
                  '@type': 'EntryPoint',
                  urlTemplate: `${SITE_URL}/search?q={search_term_string}`,
                },
                'query-input': 'required name=search_term_string',
              },
            }),
          },
          {
            tagName: 'script',
            attributes: {type: 'application/ld+json'},
            innerHTML: JSON.stringify({
              '@context': 'https://schema.org',
              '@type': 'Organization',
              name: 'Treq',
              url: SITE_URL,
              logo: `${SITE_URL}/img/favicon.svg`,
              sameAs: ['https://github.com/tznc/treq'],
            }),
          },
        ],
      };
    },
  };
};
