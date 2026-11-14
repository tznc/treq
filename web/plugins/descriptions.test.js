// @ts-check
// Search engines and llms.txt use the frontmatter description as the page
// summary. Without one, Docusaurus falls back to the first paragraph, which on
// some pages is a heading such as "Introduction".
const path = require('path');
const {collectMarkdownPages} = require('./rawMarkdownPlugin');

const pages = collectMarkdownPages(path.join(__dirname, '..'));

test.each(pages.map((p) => [p.sourcePath, p.description]))('%s has a 50-160 character description', (_source, description) => {
  expect(description.length).toBeGreaterThanOrEqual(50);
  expect(description.length).toBeLessThanOrEqual(160);
});
