// @ts-check
const jsonLdPlugin = require('./jsonLdPlugin');

const schemas = /** @type {{headTags: {innerHTML: string}[]}} */ (jsonLdPlugin().injectHtmlTags()).headTags.map((t) =>
  JSON.parse(t.innerHTML),
);
const website = schemas.find((s) => s['@type'] === 'WebSite');

// "treq" is also a Python HTTP library. alternateName gives Google the names
// people search for this site by, for the site name shown in results.
test('WebSite lists the names people search for Treq by', () => {
  expect(website.alternateName).toEqual(['Treq AI', 'treq.dev']);
});

test('WebSite description matches the current positioning', () => {
  expect(website.description).toBe('Isolates each agent and rebases stacked PRs when the base moves');
});
