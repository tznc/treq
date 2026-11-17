// @ts-check
const fs = require('fs');
const path = require('path');
const {canRedistribute, truncateDescription, relatedSkills, installCommands, triggerClause} = require('./skillPageContent');

// Registers one static route per catalogued skill (e.g. /skills/anthropic/pdf),
// rendering a shared SkillDetailPage component with that skill's data as props.
// This is the standard Docusaurus pattern for turning a data file into many
// pages without hand-writing one file per route.
/** @typedef {{id: string, name: string, url: string, stars?: number}} Source */
/** @typedef {{id: string, name: string, description: string | null, source: string, category: string | null, route: string, path?: string, license?: string | null, files?: unknown[]}} Skill */
/** @typedef {{sources: Source[], skills: Skill[]}} Catalog */
/** @typedef {Record<string, {body: string | null, licenseUrl: string | null, manualOnly: boolean}>} SkillPages */

/**
 * Author pages (e.g. /skills/mattpocock) list one source's skills. People
 * search skills by author name, so each author gets a page of its own.
 * @param {Catalog} catalog
 */
function buildAuthorPages(catalog) {
  return catalog.sources
    .map((source) => ({
      route: `/skills/${source.id}`,
      source,
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
 */
function skillPageProps(skill, catalog, pages) {
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
  };
}

/**
 * @param {import('@docusaurus/types').LoadContext} context
 * @returns {import('@docusaurus/types').Plugin<Catalog & {pages: SkillPages}>}
 */
function skillsPlugin(context) {
  return {
    name: 'skills-plugin',
    async loadContent() {
      const dataDir = path.join(context.siteDir, 'src', 'data');
      const catalogPath = path.join(dataDir, 'skills.json');
      if (!fs.existsSync(catalogPath)) return {sources: [], skills: [], pages: {}};
      const catalog = JSON.parse(fs.readFileSync(catalogPath, 'utf8'));
      const pagesPath = path.join(dataDir, 'skill-pages.json');
      const pages = fs.existsSync(pagesPath) ? JSON.parse(fs.readFileSync(pagesPath, 'utf8')) : {};
      const staticDir = path.join(context.siteDir, 'static', 'skills');
      fs.mkdirSync(staticDir, { recursive: true });
      fs.copyFileSync(catalogPath, path.join(staticDir, 'catalog.json'));
      return {...catalog, pages};
    },
    async contentLoaded({ content: {pages, ...catalog}, actions }) {
      for (const skill of catalog.skills) {
        actions.addRoute({
          path: skill.route,
          component: '@site/src/components/SkillDetailPage/index.tsx',
          exact: true,
          props: skillPageProps(skill, catalog, pages),
        });
      }
      for (const page of buildAuthorPages(catalog)) {
        actions.addRoute({
          path: page.route,
          component: '@site/src/components/SkillAuthorPage/index.tsx',
          exact: true,
          props: { source: page.source, skills: page.skills },
        });
      }
    },
  };
}

module.exports = skillsPlugin;
module.exports.buildAuthorPages = buildAuthorPages;
module.exports.skillPageTitle = skillPageTitle;
module.exports.skillPageProps = skillPageProps;
