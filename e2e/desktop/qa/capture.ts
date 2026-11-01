// The QA run's screenshot helper. `capture` writes `<name>.png` and
// `<name>.json` (what a reviewer should check on it) into `QA_OUT`
// (default `qa/.shots/`). The name gets the theme the app runs in
// (`QA_THEME`, dark by default) as a suffix unless it already ends in
// `-dark` or `-light`.
//
// `page.screenshot` sees only the page's own window. Toasts, tooltips and
// menus are popup windows of their own, so `screen: true` grabs the whole X
// screen (`import -window root`, ImageMagick) and crops it to the windows
// that show, which needs `DISPLAY` and `xdotool`.
import { execFileSync } from "node:child_process";
import { mkdirSync, writeFileSync } from "node:fs";
import { dirname, join } from "node:path";
import { fileURLToPath } from "node:url";
import type { Page } from "@playwright/test";

const here = dirname(fileURLToPath(import.meta.url));
export const outDir = process.env.QA_OUT ?? join(here, ".shots");
export const theme = process.env.QA_THEME === "light" ? "light" : "dark";

type Clip = { x: number; y: number; width: number; height: number };

export interface Capture {
  /** `NN-area-state`; the theme suffix is added. */
  name: string;
  /** Up to three visual claims a reviewer checks on the PNG. */
  expectations: string[];
  /** A title for the manifest; defaults to the name. */
  title?: string;
  /** A known issue the shot shows. */
  issue?: string;
  /** Grab the X screen, popup windows included. */
  screen?: boolean;
  /** Logical pixels of the page's window (page captures only). */
  clip?: Clip;
  /** Move the pointer off every control first (default true). */
  rest?: boolean;
}

/** Moves the pointer to the sidebar's empty foot, so no hover state shows. */
export async function rest(page: Page): Promise<void> {
  const size = page.viewportSize() ?? { width: 1400, height: 900 };
  await page.mouse.move(140, size.height - 20, { steps: 8 });
  await page.waitForTimeout(300);
}

function themed(name: string): string {
  return /-(dark|light)$/.test(name) ? name : `${name}-${theme}`;
}

/**
 * The bounding box, in device pixels, of the mapped windows but the root
 * (popups have no name, so they are found by class).
 */
function screenBox(): { x: number; y: number; w: number; h: number } | null {
  let ids: string[];
  try {
    ids = execFileSync("xdotool", ["search", "--onlyvisible", "--class", ""], {
      encoding: "utf8",
    })
      .split("\n")
      .filter(Boolean);
  } catch {
    return null;
  }
  const screen = execFileSync("xdotool", ["getdisplaygeometry"], {
    encoding: "utf8",
  })
    .trim()
    .split(" ")
    .map(Number);
  let box: { x0: number; y0: number; x1: number; y1: number } | null = null;
  for (const id of ids) {
    let geo: string;
    try {
      geo = execFileSync("xdotool", ["getwindowgeometry", "--shell", id], {
        encoding: "utf8",
      });
    } catch {
      continue;
    }
    const v = Object.fromEntries(
      geo
        .split("\n")
        .filter(Boolean)
        .map((l) => l.split("=") as [string, string]),
    );
    const [x, y, w, h] = [v.X, v.Y, v.WIDTH, v.HEIGHT].map(Number);
    if (w < 4 || h < 4) continue;
    if (w >= screen[0] && h >= screen[1]) continue;
    const r = {
      x0: Math.max(0, x),
      y0: Math.max(0, y),
      x1: Math.min(screen[0], x + w),
      y1: Math.min(screen[1], y + h),
    };
    box = box
      ? {
          x0: Math.min(box.x0, r.x0),
          y0: Math.min(box.y0, r.y0),
          x1: Math.max(box.x1, r.x1),
          y1: Math.max(box.y1, r.y1),
        }
      : r;
  }
  return box && { x: box.x0, y: box.y0, w: box.x1 - box.x0, h: box.y1 - box.y0 };
}

/** Writes the whole X screen, cropped to the app's windows, to `path`. */
export function screenShot(path: string): void {
  const box = screenBox();
  const crop = box ? ["-crop", `${box.w}x${box.h}+${box.x}+${box.y}`, "+repage"] : [];
  execFileSync("import", ["-window", "root", ...crop, path]);
}

/** Takes the shot and records what to check on it. Returns the PNG's name. */
export async function capture(page: Page, shot: Capture): Promise<string> {
  if (shot.expectations.length > 3) {
    throw new Error(`${shot.name}: at most three expectations`);
  }
  mkdirSync(outDir, { recursive: true });
  const name = themed(shot.name);
  if (shot.rest !== false) await rest(page);
  const png = join(outDir, `${name}.png`);
  let issue = shot.issue;
  if (shot.screen) {
    await page.waitForTimeout(300);
    screenShot(png);
    // A blank, mapped window over the repo window turns the X screen
    // black while the page's own screenshot still looks right.
    const mean = Number(
      execFileSync("convert", [png, "-format", "%[fx:mean]", "info:"], { encoding: "utf8" }),
    );
    if (mean < 0.02) {
      const blank = "The screen is black: a blank app window covers the repo window";
      issue = issue ? `${issue}; ${blank}` : blank;
    }
  } else {
    await page.screenshot({ path: png, clip: shot.clip });
  }
  writeFileSync(
    join(outDir, `${name}.json`),
    `${JSON.stringify(
      {
        file: `${name}.png`,
        title: shot.title ?? name,
        what_to_check: shot.expectations,
        ...(issue ? { issue } : {}),
      },
      null,
      2,
    )}\n`,
  );
  return `${name}.png`;
}
