#!/usr/bin/env node
import fs from "node:fs";
import path from "node:path";
import { fileURLToPath } from "node:url";

const __dirname = path.dirname(fileURLToPath(import.meta.url));
const landingDir = path.join(__dirname, "..", "static", "img", "landing");
const listPath = path.join(
  __dirname,
  "..",
  "src",
  "data",
  "landing-screenshots.json",
);
const files = JSON.parse(fs.readFileSync(listPath, "utf8"));
const missing = files.filter(
  (name) => !fs.existsSync(path.join(landingDir, name)),
);
const heroPath = path.join(__dirname, "..", "static", "img", "code.png");
if (!fs.existsSync(heroPath)) {
  missing.push("../code.png");
}

if (missing.length > 0) {
  console.error(
    `Missing ${missing.length} landing screenshot(s) in ${landingDir}:\n  ${missing.join("\n  ")}\n` +
      "Generate them with `npm run landing` in e2e/desktop (see its README).",
  );
  process.exit(1);
}

console.log(`All ${files.length} landing screenshots present in ${landingDir}`);
console.log(`Hero present at ${heroPath}`);
