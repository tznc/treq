// @ts-check
const fs = require('fs');
const path = require('path');
const matter = require('gray-matter');
const {canRedistribute, truncateDescription, relatedSkills, installCommands, triggerClause} = require('./skillPageContent');

// Registers one static route per catalogued skill (e.g. /skills/anthropic/pdf),
// rendering a shared SkillDetailPage component with that skill's data as props.
// This is the standard Docusaurus pattern for turning a data file into many
// pages without hand-writing one file per route.
/** @typedef {{id: string, name: string, url: string, stars?: number}} Source */
/** @typedef {{id: string, name: string, description: string | null, source: string, category: string | null, route: string, path?: string, license?: string | null, files?: unknown[]}} Skill */
/** @typedef {{sources: Source[], skills: Skill[]}} Catalog */
/** @typedef {Record<string, {body: string | null, licenseUrl: string | null, manualOnly: boolean}>} SkillPages */
/** @typedef {Record<string, {description: string, body: string}>} AuthorWriteups */

/**
 * Reads the hand-written review of each source from `<dir>/<id>.md`: a meta
 * description in the frontmatter and a markdown body that renders above the
 * author page's skill list.
 * @param {string} dir
 * @returns {AuthorWriteups}
 */
function loadAuthorWriteups(dir) {
  if (!fs.existsSync(dir)) return {};
  /** @type {AuthorWriteups} */
  const writeups = {};
  for (const file of fs.readdirSync(dir).filter((f) => f.endsWith('.md'))) {
    const {data, content} = matter(fs.readFileSync(path.join(dir, file), 'utf8'));
    writeups[path.basename(file, '.md')] = {description: data.description, body: content.trim()};
  }
  return writeups;
}

/**
 * Author pages (e.g. /skills/mattpocock) list one source's skills. People
 * search skills by author name, so each author gets a page of its own.
 * @param {Catalog} catalog
 * @param {AuthorWriteups} [writeups]
 */
function buildAuthorPages(catalog, writeups = {}) {
  return catalog.sources
    .map((source) => ({
      route: `/skills/${source.id}`,
      source,
      writeup: writeups[source.id] ?? null,
      skills: catalog.skills
        .filter((skill) => skill.source === source.id)
        .map(({files, ...rest}) => rest)
        .sort((a, b) => a.name.localeCompare(b.name)),
    }))
    .filter((page) => page.skills.length > 0);
}

/**
 * @param {Skill} skill
 * @param {Source[]} sources
 */
function skillPageTitle(skill, sources) {
  const author = sources.find((source) => source.id === skill.source)?.name ?? skill.source;
  return `${skill.name} skill by ${author}`;
}

/**
 * Props for one skill page. The SKILL.md body is passed only when the skill's
 * license permits redistribution, so the page renders it into static HTML
 * with credit to the author.
 * @param {Skill} skill
 * @param {Catalog} catalog
 * @param {SkillPages} pages SKILL.md bodies and frontmatter flags, by skill id
 * @param {Set<string>} [indexed] routes kept in search (skills-indexed.json); every other skill page is noindex
 */
function skillPageProps(skill, catalog, pages, indexed = new Set()) {
  const source = catalog.sources.find((s) => s.id === skill.source);
  const author = source?.name ?? skill.source;
  const page = pages[skill.id];
  return {
    skill,
    title: skillPageTitle(skill, catalog.sources),
    metaDescription: skill.description
      ? truncateDescription(skill.description)
      : truncateDescription(`${skill.name} is an agent skill by ${author}. Install it in Claude Code, Codex or Treq.`),
    author,
    authorRoute: source ? `/skills/${source.id}` : null,
    body: page && canRedistribute(skill.license) ? page.body : null,
    licenseUrl: page?.licenseUrl ?? null,
    manualOnly: page?.manualOnly ?? false,
    trigger: triggerClause(skill.description),
    install: source && skill.path ? installCommands({path: skill.path}, source) : null,
    related: relatedSkills(skill, catalog.skills, catalog.sources),
    noindex: !indexed.has(skill.route),
  };
}

/**
 * @param {import('@docusaurus/types').LoadContext} context
 * @returns {import('@docusaurus/types').Plugin<Catalog & {pages: SkillPages, writeups: AuthorWriteups, indexed: string[]}>}
 */
function skillsPlugin(context) {
  return {
    name: 'skills-plugin',
    async loadContent() {
      const dataDir = path.join(context.siteDir, 'src', 'data');
      const catalogPath = path.join(dataDir, 'skills.json');
      if (!fs.existsSync(catalogPath)) return {sources: [], skills: [], pages: {}, writeups: {}, indexed: []};
      const catalog = JSON.parse(fs.readFileSync(catalogPath, 'utf8'));
      const pagesPath = path.join(dataDir, 'skill-pages.json');
      const pages = fs.existsSync(pagesPath) ? JSON.parse(fs.readFileSync(pagesPath, 'utf8')) : {};
      const staticDir = path.join(context.siteDir, 'static', 'skills');
      fs.mkdirSync(staticDir, { recursive: true });
      fs.copyFileSync(catalogPath, path.join(staticDir, 'catalog.json'));
      const writeups = loadAuthorWriteups(path.join(context.siteDir, 'skill-authors'));
      const {routes: indexed} = JSON.parse(fs.readFileSync(path.join(context.siteDir, 'skills-indexed.json'), 'utf8'));
      return {...catalog, pages, writeups, indexed};
    },
    async contentLoaded({ content: {pages, writeups, indexed, ...catalog}, actions }) {
      const indexedRoutes = new Set(indexed);
      for (const skill of catalog.skills) {
        actions.addRoute({
          path: skill.route,
          component: '@site/src/components/SkillDetailPage/index.tsx',
          exact: true,
          props: skillPageProps(skill, catalog, pages, indexedRoutes),
        });
      }
      for (const page of buildAuthorPages(catalog, writeups)) {
        actions.addRoute({
          path: page.route,
          component: '@site/src/components/SkillAuthorPage/index.tsx',
          exact: true,
          props: { source: page.source, skills: page.skills, writeup: page.writeup },
        });
      }
    },
  };
}

module.exports = skillsPlugin;
module.exports.buildAuthorPages = buildAuthorPages;
module.exports.loadAuthorWriteups = loadAuthorWriteups;
module.exports.skillPageTitle = skillPageTitle;
module.exports.skillPageProps = skillPageProps;
