// @ts-check
const fs = require('fs');
const path = require('path');

const CONTENT_SOURCES = [
  {contentPath: 'docs', routeBasePath: 'docs'},
  {contentPath: 'learn', routeBasePath: 'learn'},
];

/**
 * @param {string} raw
 * @returns {string}
 */
function normalizeMarkdown(raw) {
  return raw
    .replace(/^---[\s\S]*?---\n?/, '')
    .replace(/<!--[\s\S]*?-->/g, '')
    .replace(/\{\/\*[\s\S]*?\*\/\}/g, '')
    .replace(/<[\s\S]*?>/g, '')
    .replace(/\n{3,}/g, '\n\n')
    .trim();
}

/** @typedef {{contentPath: string, routeBasePath: string}} ContentSource */
/** @typedef {{sourcePath: string, outputPath: string, content: string}} MarkdownPage */

/**
 * @param {ContentSource} source
 * @param {string} relativePath
 * @returns {string}
 */
function markdownOutputPath({contentPath, routeBasePath}, relativePath) {
  const parsed = path.parse(relativePath);
  const withoutExtension = path.posix.join(parsed.dir, parsed.name);
  const routePath = parsed.name === 'index' ? parsed.dir : withoutExtension;
  const outputSegments = [routeBasePath, routePath].filter(Boolean);
  return `${path.posix.join(...outputSegments)}.md`;
}

/**
 * @param {string} siteDir
 * @param {ContentSource[]} [sources]
 * @returns {MarkdownPage[]}
 */
function collectMarkdownPages(siteDir, sources = CONTENT_SOURCES) {
  /** @type {MarkdownPage[]} */
  const pages = [];

  for (const source of sources) {
    const rootDir = path.join(siteDir, source.contentPath);
    if (!fs.existsSync(rootDir)) continue;

    /** @param {string} dir */
    function processDir(dir) {
      const entries = fs.readdirSync(dir, {withFileTypes: true});
      for (const entry of entries) {
        const fullPath = path.join(dir, entry.name);
        if (entry.isDirectory()) {
          processDir(fullPath);
          continue;
        }

        if (!entry.name.endsWith('.md') && !entry.name.endsWith('.mdx')) continue;

        const relativePath = path.relative(rootDir, fullPath).split(path.sep).join(path.posix.sep);
        const raw = fs.readFileSync(fullPath, 'utf8');
        pages.push({
          sourcePath: path.relative(siteDir, fullPath).split(path.sep).join(path.posix.sep),
          outputPath: markdownOutputPath(source, relativePath),
          content: normalizeMarkdown(raw),
        });
      }
    }

    processDir(rootDir);
  }

  return pages;
}

/**
 * @param {import('@docusaurus/types').LoadContext} context
 * @returns {import('@docusaurus/types').Plugin<MarkdownPage[]>}
 */
function rawMarkdownPlugin(context) {
  return {
    name: 'raw-markdown-plugin',
    async loadContent() {
      return collectMarkdownPages(context.siteDir);
    },
    async contentLoaded({content, actions}) {
      actions.setGlobalData(
        Object.fromEntries(content.map((page) => [page.sourcePath, page.content])),
      );
    },
    async postBuild({outDir, content}) {
      for (const page of content) {
        const outputFile = path.join(outDir, page.outputPath);
        fs.mkdirSync(path.dirname(outputFile), {recursive: true});
        fs.writeFileSync(outputFile, `${page.content}\n`, 'utf8');
      }
    },
  };
}

module.exports = rawMarkdownPlugin;
module.exports.normalizeMarkdown = normalizeMarkdown;
module.exports.markdownOutputPath = markdownOutputPath;
module.exports.collectMarkdownPages = collectMarkdownPages;
