#!/usr/bin/env node
/**
 * Puts hero + landing PNGs into web/static/img so Docusaurus can copy them.
 *
 * Order:
 * 1. Copy every PNG from an optional source tree (a fresh capture).
 * 2. If the hero is still missing, copy assets/screenshots/code.png.
 * 3. Copy committed crops from assets/screenshots/landing/ that are still
 *    missing.
 * 4. If a landing crop is still missing, copy the hero so /img/landing/*.png
 *    never 404s when capture did not run.
 */
import fs from "node:fs";
import path from "node:path";
import { fileURLToPath } from "node:url";

const __dirname = path.dirname(fileURLToPath(import.meta.url));
const webRoot = path.join(__dirname, "..");
const repoRoot = path.join(webRoot, "..");
const destImg = path.join(webRoot, "static", "img");
const destLanding = path.join(destImg, "landing");
const destHero = path.join(destImg, "code.png");
const committedHero = path.join(repoRoot, "assets", "screenshots", "code.png");
const committedLanding = path.join(repoRoot, "assets", "screenshots", "landing");
const listPath = path.join(webRoot, "src", "data", "landing-screenshots.json");
const names = JSON.parse(fs.readFileSync(listPath, "utf8"));
const srcRoot = process.argv[2];

function walk(dir, acc = []) {
  if (!fs.existsSync(dir)) return acc;
  for (const ent of fs.readdirSync(dir, { withFileTypes: true })) {
    const next = path.join(dir, ent.name);
    if (ent.isDirectory()) walk(next, acc);
    else acc.push(next);
  }
  return acc;
}

function copyFile(from, to) {
  fs.mkdirSync(path.dirname(to), { recursive: true });
  fs.copyFileSync(from, to);
}

fs.mkdirSync(destLanding, { recursive: true });

function findNamed(root, filename) {
  return walk(root).find((file) => path.basename(file) === filename);
}

let fromArtifact = 0;
if (srcRoot) {
  const hero = findNamed(srcRoot, "code.png");
  if (hero) {
    copyFile(hero, destHero);
    fromArtifact += 1;
  }
  for (const name of names) {
    const found = findNamed(srcRoot, name);
    if (!found) continue;
    copyFile(found, path.join(destLanding, name));
    fromArtifact += 1;
  }
  console.log(`Copied ${fromArtifact} PNG(s) from ${srcRoot}`);
}

if (!fs.existsSync(destHero)) {
  if (!fs.existsSync(committedHero)) {
    console.error(`Missing hero screenshot at ${committedHero}`);
    process.exit(1);
  }
  copyFile(committedHero, destHero);
  console.log(`Staged hero from ${committedHero}`);
}

for (const name of names) {
  const dest = path.join(destLanding, name);
  const committed = path.join(committedLanding, name);
  if (fs.existsSync(dest) || !fs.existsSync(committed)) continue;
  copyFile(committed, dest);
  console.log(`Staged ${name} from ${committedLanding}`);
}

let filled = 0;
for (const name of names) {
  const dest = path.join(destLanding, name);
  if (fs.existsSync(dest)) continue;
  copyFile(destHero, dest);
  filled += 1;
}

if (filled > 0) {
  console.log(
    `Filled ${filled} missing landing crop(s) from the hero so preview URLs resolve`,
  );
}

console.log(`Hero: ${destHero}`);
console.log(`Landing: ${destLanding} (${names.length} files)`);
