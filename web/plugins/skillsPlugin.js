// @ts-check
const fs = require('fs');
const path = require('path');

// Registers one static route per catalogued skill (e.g. /skills/anthropic/pdf),
// rendering a shared SkillDetailPage component with that skill's data as props.
// This is the standard Docusaurus pattern for turning a data file into many
// pages without hand-writing one file per route.
/** @typedef {{id: string, name: string, url: string, stars?: number}} Source */
/** @typedef {{id: string, name: string, description: string | null, source: string, category: string | null, route: string, files?: unknown[]}} Skill */
/** @typedef {{sources: Source[], skills: Skill[]}} Catalog */

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
 * @param {import('@docusaurus/types').LoadContext} context
 * @returns {import('@docusaurus/types').Plugin<Catalog>}
 */
function skillsPlugin(context) {
  return {
    name: 'skills-plugin',
    async loadContent() {
      const catalogPath = path.join(context.siteDir, 'src', 'data', 'skills.json');
      if (!fs.existsSync(catalogPath)) return {sources: [], skills: []};
      const catalog = JSON.parse(fs.readFileSync(catalogPath, 'utf8'));
      const staticDir = path.join(context.siteDir, 'static', 'skills');
      fs.mkdirSync(staticDir, { recursive: true });
      fs.copyFileSync(catalogPath, path.join(staticDir, 'catalog.json'));
      return catalog;
    },
    async contentLoaded({ content: catalog, actions }) {
      for (const skill of catalog.skills) {
        const source = catalog.sources.find((s) => s.id === skill.source);
        actions.addRoute({
          path: skill.route,
          component: '@site/src/components/SkillDetailPage/index.tsx',
          exact: true,
          props: {
            skill,
            title: skillPageTitle(skill, catalog.sources),
            authorRoute: source ? `/skills/${source.id}` : null,
          },
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
