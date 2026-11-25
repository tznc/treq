// Splits build/sitemap.xml into one sitemap per section (plugins/splitSitemap.js).
import fs from 'node:fs';
import path from 'node:path';
import {createRequire} from 'node:module';

const {splitSitemap} = createRequire(import.meta.url)('../plugins/splitSitemap.js');

const buildDir = path.resolve(process.argv[2] ?? 'build');
const sitemapPath = path.join(buildDir, 'sitemap.xml');
const {index, files} = splitSitemap(fs.readFileSync(sitemapPath, 'utf8'), 'https://treq.dev');
for (const [name, xml] of Object.entries(files)) fs.writeFileSync(path.join(buildDir, name), xml, 'utf8');
fs.writeFileSync(sitemapPath, index, 'utf8');
console.log(`split sitemap.xml into ${Object.keys(files).join(', ')}`);
