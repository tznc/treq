// @ts-check
// With trailingSlash: true, a relative link such as ./other-page resolves
// against /section/this-page/ and points at a child path that does not exist.
// Relative links must name the file (./other-page.mdx) so Docusaurus resolves it.
const path = require('path');
const {collectMarkdownPages} = require('./rawMarkdownPlugin');

const pages = collectMarkdownPages(path.join(__dirname, '..'));

test.each(pages.map((p) => [p.sourcePath, p.content]))('%s has no extensionless relative links', (_source, content) => {
  expect(content.match(/\]\(\.\/[A-Za-z0-9_-]+\/?(#[^)]*)?\)/g) ?? []).toEqual([]);
});
