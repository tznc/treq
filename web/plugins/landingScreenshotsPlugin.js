// @ts-check
const {spawnSync} = require('child_process');
const fs = require('fs');
const path = require('path');

/**
 * @param {string} src
 * @param {string} dest
 * @returns {number}
 */
function copyPngTree(src, dest) {
  if (!fs.existsSync(src)) return 0;
  fs.mkdirSync(dest, {recursive: true});
  let copied = 0;
  for (const ent of fs.readdirSync(src, {withFileTypes: true})) {
    const from = path.join(src, ent.name);
    const to = path.join(dest, ent.name);
    if (ent.isDirectory()) {
      copied += copyPngTree(from, to);
      continue;
    }
    if (!ent.name.endsWith('.png')) continue;
    fs.copyFileSync(from, to);
    copied += 1;
  }
  return copied;
}

/**
 * @param {import('@docusaurus/types').LoadContext} context
 * @returns {import('@docusaurus/types').Plugin<void>}
 */
function landingScreenshotsPlugin(context) {
  return {
    name: 'landing-screenshots-plugin',
    async loadContent() {
      if (process.env.SKIP_LANDING_SCREENSHOTS === '1') {
        console.warn(
          '[landing-screenshots] skipped staging (SKIP_LANDING_SCREENSHOTS=1). Using files already in static/img.',
        );
        return;
      }
      if (process.env.NODE_ENV !== 'production') {
        return;
      }

      // The landing PNGs are generated (e2e/desktop/landing), never
      // committed: stage the hero, then fail the build on a missing PNG.
      for (const script of [
        'scripts/stage-landing-screenshots.mjs',
        'scripts/assert-landing-screenshots.mjs',
      ]) {
        const result = spawnSync('node', [script], {
          cwd: context.siteDir,
          stdio: 'inherit',
          env: process.env,
          shell: false,
        });
        if (result.status !== 0) {
          throw new Error(`[landing-screenshots] ${script} exited ${result.status}`);
        }
      }
    },
    async postBuild({outDir}) {
      const copied = copyPngTree(
        path.join(context.siteDir, 'static', 'img'),
        path.join(outDir, 'img'),
      );
      console.log(
        `[landing-screenshots] copied ${copied} PNG(s) into ${path.join(outDir, 'img')}`,
      );
    },
  };
}

module.exports = landingScreenshotsPlugin;
