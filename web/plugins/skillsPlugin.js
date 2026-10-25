// @ts-check
const fs = require('fs');
const path = require('path');

// Registers one static route per catalogued skill (e.g. /skills/anthropic/pdf),
// rendering a shared SkillDetailPage component with that skill's data as props.
// This is the standard Docusaurus pattern for turning a data file into many
// pages without hand-writing one file per route.
/**
 * @param {import('@docusaurus/types').LoadContext} context
 * @returns {import('@docusaurus/types').Plugin<Array<{route: string}>>}
 */
function skillsPlugin(context) {
  return {
    name: 'skills-plugin',
    async loadContent() {
      const catalogPath = path.join(context.siteDir, 'src', 'data', 'skills.json');
      if (!fs.existsSync(catalogPath)) return [];
      const catalog = JSON.parse(fs.readFileSync(catalogPath, 'utf8'));
      const staticDir = path.join(context.siteDir, 'static', 'skills');
      fs.mkdirSync(staticDir, { recursive: true });
      fs.copyFileSync(catalogPath, path.join(staticDir, 'catalog.json'));
      return catalog.skills;
    },
    async contentLoaded({ content: skills, actions }) {
      for (const skill of skills) {
        actions.addRoute({
          path: skill.route,
          component: '@site/src/components/SkillDetailPage/index.tsx',
          exact: true,
          props: { skill },
        });
      }
    },
  };
}

module.exports = skillsPlugin;
