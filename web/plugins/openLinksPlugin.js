// @ts-check
// /open/pr/{owner}/{repo}/{number}: one page for every PR, so it matches any
// path under /open/pr (exact: false). Cloudflare serves its HTML for those
// paths through the rewrite in static/_redirects.
/** @type {import('@docusaurus/types').PluginModule} */
module.exports = function openLinksPlugin() {
  return {
    name: 'open-links',
    async contentLoaded({actions}) {
      actions.addRoute({
        path: '/open/pr/',
        exact: false,
        component: '@site/src/components/OpenPrPage.tsx',
      });
    },
  };
};
