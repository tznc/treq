// @ts-check
const {spawnSync} = require('child_process');
const fs = require('fs');
const path = require('path');

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

/** @type {import('@docusaurus/types').PluginModule} */
function landingScreenshotsPlugin(context) {
  return {
    name: 'landing-screenshots-plugin',
    async loadContent() {
      if (process.env.SKIP_LANDING_SCREENSHOTS === '1') {
        console.warn(
          '[landing-screenshots] skipped capture (SKIP_LANDING_SCREENSHOTS=1). Using files already in static/img.',
        );
        return;
      }
      if (process.env.NODE_ENV !== 'production') {
        return;
      }

      const landingDir = path.join(context.siteDir, 'static', 'img', 'landing');
      const listPath = path.join(
        context.siteDir,
        'src',
        'data',
        'landing-screenshots.json',
      );
      const files = JSON.parse(fs.readFileSync(listPath, 'utf8'));
      fs.mkdirSync(landingDir, {recursive: true});

      const missing = files.filter(
        (name) => !fs.existsSync(path.join(landingDir, name)),
      );
      if (missing.length === 0 && process.env.FORCE_LANDING_SCREENSHOTS !== '1') {
        return;
      }

      // Copies the committed screenshots (assets/screenshots/) into place.
      const result = spawnSync('node', ['scripts/stage-landing-screenshots.mjs'], {
        cwd: context.siteDir,
        stdio: 'inherit',
        env: process.env,
        shell: false,
      });
      if (result.status !== 0) {
        const stillMissing = files.filter(
          (name) => !fs.existsSync(path.join(landingDir, name)),
        );
        if (stillMissing.length > 0) {
          console.warn(
            `[landing-screenshots] staging exited ${result.status}. Missing: ${stillMissing.join(', ')}.`,
          );
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
