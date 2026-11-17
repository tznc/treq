// @ts-check
const fs = require('fs');
const path = require('path');

const CONTENT_SOURCES = [
  {contentPath: 'docs', routeBasePath: 'docs'},
  {contentPath: 'learn', routeBasePath: 'learn'},
  {contentPath: 'compare', routeBasePath: 'compare'},
];

/**
 * @param {string} raw
 * @returns {Record<string, string>}
 */
function parseFrontMatter(raw) {
  const match = raw.match(/^---\n([\s\S]*?)\n---/);
  if (!match) return {};
  return Object.fromEntries(
    match[1]
      .split('\n')
      .map((line) => line.match(/^(\w+):\s*(.*)$/))
      .filter((m) => m !== null)
      .map((m) => [m[1], m[2].replace(/^['"]|['"]$/g, '')]),
  );
}

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
/** @typedef {{sourcePath: string, outputPath: string, content: string, title: string, description: string, section: string}} MarkdownPage */

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
        const frontMatter = parseFrontMatter(raw);
        if (frontMatter.draft === 'true') continue;
        const content = normalizeMarkdown(raw);
        pages.push({
          sourcePath: path.relative(siteDir, fullPath).split(path.sep).join(path.posix.sep),
          outputPath: markdownOutputPath(source, relativePath),
          content,
          title: frontMatter.title || content.match(/^# (.+)$/m)?.[1] || path.parse(entry.name).name,
          description: frontMatter.description || '',
          section: source.routeBasePath,
        });
      }
    }

    processDir(rootDir);
  }

  return pages;
}

/** @typedef {{title: string, description: string, url: string}} SiteInfo */

/**
 * @param {MarkdownPage[]} pages
 * @param {SiteInfo} site
 * @returns {string}
 */
function buildLlmsTxt(pages, {title, description, url}) {
  const sections = CONTENT_SOURCES.map(({routeBasePath}) => {
    const links = pages
      .filter((page) => page.section === routeBasePath)
      .sort((a, b) => a.outputPath.localeCompare(b.outputPath))
      .map((page) => {
        const suffix = page.description ? `: ${page.description}` : '';
        return `- [${page.title}](${url}/${page.outputPath})${suffix}`;
      });
    const heading = routeBasePath[0].toUpperCase() + routeBasePath.slice(1);
    return links.length ? `## ${heading}\n\n${links.join('\n')}\n` : '';
  });
  return [`# ${title}\n\n> ${description}\n`, ...sections.filter(Boolean)].join('\n');
}

/**
 * @param {MarkdownPage[]} pages
 * @param {SiteInfo} site
 * @returns {string}
 */
function buildLlmsFullTxt(pages, {title, description, url}) {
  const bodies = pages.map((page) => `---\nSource: ${url}/${page.outputPath}\n\n${page.content}\n`);
  return [`# ${title}\n\n> ${description}\n`, ...bodies].join('\n');
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
      const {title, tagline, url} = context.siteConfig;
      const site = {title, description: tagline, url};
      fs.writeFileSync(path.join(outDir, 'llms.txt'), buildLlmsTxt(content, site), 'utf8');
      fs.writeFileSync(path.join(outDir, 'llms-full.txt'), buildLlmsFullTxt(content, site), 'utf8');
    },
  };
}

module.exports = rawMarkdownPlugin;
module.exports.normalizeMarkdown = normalizeMarkdown;
module.exports.markdownOutputPath = markdownOutputPath;
module.exports.collectMarkdownPages = collectMarkdownPages;
module.exports.buildLlmsTxt = buildLlmsTxt;
module.exports.buildLlmsFullTxt = buildLlmsFullTxt;
