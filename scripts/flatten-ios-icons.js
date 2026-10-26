#!/usr/bin/env node
/**
 * Flattens the iOS app icons onto white and rewrites them without an alpha
 * channel. `tauri icon` writes them as RGBA (with a few alpha-254 pixels at
 * the edges), and App Store Connect rejects an app icon with an alpha channel
 * (ITMS-90717). Runs after `tauri icon` in `npm run icons:generate`.
 */

import fs from 'fs';
import path from 'path';
import { fileURLToPath } from 'url';
import { PNG } from 'pngjs';

const __dirname = path.dirname(fileURLToPath(import.meta.url));
const iconsDir = path.join(path.dirname(__dirname), 'src-tauri', 'icons', 'ios');

const files = fs.readdirSync(iconsDir).filter((f) => f.endsWith('.png'));
for (const file of files) {
  const iconPath = path.join(iconsDir, file);
  const png = PNG.sync.read(fs.readFileSync(iconPath));
  // Composite each pixel over white.
  for (let i = 0; i < png.data.length; i += 4) {
    const alpha = png.data[i + 3] / 255;
    for (let c = 0; c < 3; c++) {
      png.data[i + c] = Math.round(png.data[i + c] * alpha + 255 * (1 - alpha));
    }
    png.data[i + 3] = 255;
  }
  // colorType 2: RGB, no alpha channel.
  fs.writeFileSync(iconPath, PNG.sync.write(png, { colorType: 2 }));
}

console.log(`✓ Flattened ${files.length} iOS icons onto white (no alpha channel)`);
