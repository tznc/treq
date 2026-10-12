#!/usr/bin/env node
/**
 * Compiles the app's real Tailwind stylesheet (the same src/index.css used by
 * `vite build`) into a single static CSS file the screenshot harness can
 * inline next to a jsdom-serialized DOM snapshot. jsdom never paints, so the
 * snapshot has no visual styling of its own -- this file is what lets a real
 * browser render it correctly.
 */
import fs from "node:fs";
import path from "node:path";
import { fileURLToPath } from "node:url";
import postcss from "postcss";
import tailwindcss from "tailwindcss";
import autoprefixer from "autoprefixer";

const __dirname = path.dirname(fileURLToPath(import.meta.url));
const repoRoot = path.resolve(__dirname, "..", "..");
const outDir = path.join(__dirname, ".generated");
const outFile = path.join(outDir, "app.css");

const inputCssPath = path.join(repoRoot, "src", "index.css");
const xtermCssPath = path.join(
	repoRoot,
	"node_modules",
	"@xterm",
	"xterm",
	"css",
	"xterm.css",
);
const prismThemeCssPath = path.join(
	repoRoot,
	"node_modules",
	"prism-themes",
	"themes",
	"prism-vs.css",
);

fs.mkdirSync(outDir, { recursive: true });

const inputCss = fs.readFileSync(inputCssPath, "utf8");

const result = await postcss([
	tailwindcss({ config: path.join(repoRoot, "tailwind.config.js") }),
	autoprefixer,
]).process(inputCss, { from: inputCssPath, to: outFile });

const xtermCss = fs.existsSync(xtermCssPath)
	? fs.readFileSync(xtermCssPath, "utf8")
	: "";
const prismThemeCss = fs.existsSync(prismThemeCssPath)
	? fs.readFileSync(prismThemeCssPath, "utf8")
	: "";

// Captures are rasterized from a file:// page, where the app's server-absolute
// /fonts/*.woff2 URLs would resolve against the filesystem root and 404. Point
// them at the real files under src/public so screenshots show the webfonts the
// app actually ships with instead of silently falling back to a system font.
const fontsDir = path.join(repoRoot, "src", "public", "fonts");
const css = result.css.replace(
	/url\((["']?)\/fonts\//g,
	(_match, quote) => `url(${quote}file://${fontsDir}/`,
);

fs.writeFileSync(
	outFile,
	`${xtermCss}\n${prismThemeCss}\n${css}\n`,
);

console.log(`Wrote compiled CSS -> ${path.relative(repoRoot, outFile)}`);
