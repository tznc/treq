/**
 * Rasterizes a jsdom-rendered DOM snapshot into a PNG.
 *
 * jsdom (used by the integration test harness) drives real app logic and
 * produces a real DOM, but never paints pixels. This takes the serialized
 * document, inlines the app's compiled Tailwind CSS, and hands the resulting
 * static HTML to a real headless browser (Chromium via playwright-core) just
 * to rasterize it.
 *
 * Every capture also carries `expectations`: plain-English claims about what
 * the rendered image should show (not DOM/testing-library assertions -- the
 * spec's own screen.findBy and expect calls already prove the DOM state
 * before capture). These are written to a `<name>.json` manifest next to the
 * PNG so an agent doing visual QA has a concrete checklist to verify against
 * the picture -- read the PNG, then confirm or refute each expectation by
 * looking at it.
 */
import fs from "node:fs";
import path from "node:path";
import { chromium } from "playwright-core";

const GENERATED_DIR = path.join(__dirname, ".generated");
const CSS_PATH = path.join(GENERATED_DIR, "app.css");
const REPO_ROOT = path.resolve(__dirname, "..", "..");
const DEFAULT_CHROMIUM_CANDIDATES = [
  "/opt/pw-browsers/chromium-1194/chrome-linux/chrome",
  path.join(
    process.env.HOME ?? "",
    ".cache/ms-playwright/chromium-1234/chrome-linux64/chrome",
  ),
];

function resolveChromiumExecutable(): string {
  if (process.env.PLAYWRIGHT_CHROMIUM_PATH) {
    return process.env.PLAYWRIGHT_CHROMIUM_PATH;
  }
  for (const candidate of DEFAULT_CHROMIUM_CANDIDATES) {
    if (candidate && fs.existsSync(candidate)) {
      return candidate;
    }
  }
  throw new Error(
    "No Chromium executable found. Set PLAYWRIGHT_CHROMIUM_PATH or install " +
      "Playwright Chromium (`npx playwright install chromium`).",
  );
}

export type CaptureOptions = {
  /** Base file name (no extension) for the .html/.png/.json written under .generated/ */
  name: string;
  viewport?: { width: number; height: number };
  /**
   * Device pixel ratio for the rasterized PNG. Marketing/README shots use 2
   * to match retina assets under assets/screenshots/.
   */
  deviceScaleFactor?: number;
  /**
   * Non-empty: what a viewer should be able to confirm by looking at this
   * screenshot, e.g. "The 'Push' button is not visible in the
   * header" or "A green toast reading 'Pushed to remote' is shown". These
   * are checked visually against the PNG, not against the DOM.
   */
  expectations: string[];
  /**
   * CSS selector for an element to scroll into view before rasterizing.
   * jsdom's scrollTop never carries into the re-rendered static HTML, so a
   * target sitting below the fold of a scrollable region (e.g. the cmdk
   * list's `overflow-y-auto`) is otherwise clipped out of the screenshot
   * even though it's present in the DOM.
   */
  scrollIntoView?: string;
  /**
   * Optional CSS selector whose bounding box is captured instead of the full
   * viewport. Used for docs crops (button + menu) when a full-window shot
   * would bury the control.
   */
  clipSelector?: string;
  /**
   * CSS selector Playwright hovers before rasterizing, so CSS `:hover` and
   * `group-hover` styles appear in the PNG. jsdom `userEvent.hover` does not
   * survive serialization into this static HTML.
   */
  hoverSelector?: string;
  /**
   * Repo-relative or absolute path, or list of paths, to also write the PNG.
   * Landing crops publish to `web/static/img/landing/*.png`.
   * The README hero publishes to `assets/screenshots/code.png`.
   * The site hero publishes to `web/static/img/code.png`.
   * Docs crops publish to `web/static/img/docs/*.png`.
   */
  publishTo?: string | string[];
};

// xterm sizes its rows from measured character cells. jsdom measures every
// cell as 0x0, so the rows it writes have no height and Chromium paints an
// empty terminal. Give the rows a real line box so terminal text shows up.
const XTERM_CAPTURE_CSS = `
.xterm .xterm-screen { width: auto !important; height: auto !important; }
.xterm .xterm-rows > div { height: 17px !important; line-height: 17px !important; width: auto !important; }
.xterm .xterm-rows span { height: auto !important; }
`;

export async function captureDocument(
  doc: Document,
  options: CaptureOptions,
): Promise<string> {
  const {
    name,
    viewport = { width: 1440, height: 900 },
    deviceScaleFactor = 1,
    expectations,
    scrollIntoView,
    clipSelector,
    hoverSelector,
    publishTo,
  } = options;

  if (!expectations || expectations.length === 0) {
    throw new Error(
      `captureDocument("${name}") requires a non-empty "expectations" list -- ` +
        "plain-English claims about what this screenshot should show, for an " +
        "agent to verify visually. See the app-qa skill.",
    );
  }

  if (!fs.existsSync(CSS_PATH)) {
    throw new Error(
      `Missing compiled CSS at ${CSS_PATH}. Run \`npm run screenshot:css\` first.`,
    );
  }
  const css = fs.readFileSync(CSS_PATH, "utf8");

  // innerHTML omits the selected attribute on <option>, so Chromium would
  // paint the first option even when jsdom's select.value is correct.
  for (const select of doc.querySelectorAll("select")) {
    const current = (select as HTMLSelectElement).value;
    for (const option of Array.from((select as HTMLSelectElement).options)) {
      if (option.value === current) {
        option.setAttribute("selected", "");
      } else {
        option.removeAttribute("selected");
      }
    }
  }
  // Same for checkboxes and radios: `checked` is a property, so innerHTML
  // would paint every box unchecked.
  for (const input of doc.querySelectorAll<HTMLInputElement>(
    'input[type="checkbox"], input[type="radio"]',
  )) {
    input.toggleAttribute("checked", input.checked);
  }

  const htmlClass = doc.documentElement.className;
  const bodyHtml = doc.body.innerHTML;

  const html = `<!doctype html>
<html class="${htmlClass}">
<head>
<meta charset="utf-8" />
<style>
html, body { margin: 0; padding: 0; }
${XTERM_CAPTURE_CSS}
${css}
</style>
</head>
<body class="${doc.body.className}">${bodyHtml}</body>
</html>`;

  fs.mkdirSync(GENERATED_DIR, { recursive: true });
  const htmlPath = path.join(GENERATED_DIR, `${name}.html`);
  const pngPath = path.join(GENERATED_DIR, `${name}.png`);
  const manifestPath = path.join(GENERATED_DIR, `${name}.json`);
  fs.writeFileSync(htmlPath, html);

  const browser = await chromium.launch({
    executablePath: resolveChromiumExecutable(),
  });
  try {
    const page = await browser.newPage({
      viewport,
      deviceScaleFactor,
    });
    await page.goto(`file://${htmlPath}`);
    if (scrollIntoView) {
      await page.locator(scrollIntoView).first().scrollIntoViewIfNeeded();
    }
    if (hoverSelector) {
      await page.locator(hoverSelector).first().hover();
    }
    if (clipSelector) {
      const locator = page.locator(clipSelector).first();
      await locator.waitFor({ state: "visible" });
      await locator.screenshot({ path: pngPath });
    } else {
      await page.screenshot({ path: pngPath });
    }
  } finally {
    await browser.close();
  }

  const destinations = publishTo
    ? (Array.isArray(publishTo) ? publishTo : [publishTo])
    : [];
  for (const destPath of destinations) {
    const dest = path.isAbsolute(destPath)
      ? destPath
      : path.join(REPO_ROOT, destPath);
    fs.mkdirSync(path.dirname(dest), { recursive: true });
    fs.copyFileSync(pngPath, dest);
  }

  fs.writeFileSync(
    manifestPath,
    JSON.stringify(
      {
        name,
        capturedAt: new Date().toISOString(),
        expectations,
        ...(publishTo ? { publishTo } : {}),
      },
      null,
      2,
    ),
  );

  return pngPath;
}

/** Place two published PNGs on a labeled board for isolation copy. */
export async function stitchPngsSideBySide(
  leftPath: string,
  rightPath: string,
  dest: string,
  labels: [string, string],
): Promise<void> {
  const leftAbs = path.isAbsolute(leftPath)
    ? leftPath
    : path.join(REPO_ROOT, leftPath);
  const rightAbs = path.isAbsolute(rightPath)
    ? rightPath
    : path.join(REPO_ROOT, rightPath);
  const destAbs = path.isAbsolute(dest) ? dest : path.join(REPO_ROOT, dest);

  const browser = await chromium.launch({
    executablePath: resolveChromiumExecutable(),
  });
  try {
    const page = await browser.newPage({
      viewport: { width: 1440, height: 900 },
      deviceScaleFactor: 2,
    });
    const html = `<!doctype html>
<html><head><meta charset="utf-8" />
<style>
  html, body { margin: 0; background: #111; color: #e5e5e5; font-family: ui-sans-serif, system-ui, sans-serif; }
  .board { display: grid; grid-template-columns: 1fr 1fr; gap: 16px; padding: 20px; }
  figure { margin: 0; }
  figcaption { font-size: 13px; font-weight: 700; margin-bottom: 8px; font-family: ui-monospace, Menlo, monospace; }
  img { width: 100%; height: auto; border-radius: 8px; border: 1px solid #333; display: block; }
</style></head>
<body>
  <div class="board">
    <figure>
      <figcaption>${labels[0]}</figcaption>
      <img src="file://${leftAbs}" />
    </figure>
    <figure>
      <figcaption>${labels[1]}</figcaption>
      <img src="file://${rightAbs}" />
    </figure>
  </div>
</body></html>`;
    const htmlPath = path.join(GENERATED_DIR, "stitch-isolation.html");
    fs.mkdirSync(GENERATED_DIR, { recursive: true });
    fs.writeFileSync(htmlPath, html);
    await page.goto(`file://${htmlPath}`);
    fs.mkdirSync(path.dirname(destAbs), { recursive: true });
    await page.screenshot({ path: destAbs, fullPage: true });
  } finally {
    await browser.close();
  }
}
