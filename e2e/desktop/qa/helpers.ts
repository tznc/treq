// Navigation the QA specs share. Every spec starts from `ready`, so one
// area can run alone (`--grep`).
import { execFileSync } from "node:child_process";
import { expect, type Page } from "@playwright/test";
import { mod } from "../fixture.ts";

export { mod };

const scale = Number(process.env.GDK_SCALE ?? "2");

/** A sidebar row: a workspace (by branch) or a home row (by repo name). */
export function row(page: Page, name: string) {
  return page.getByRole("button", { name, exact: true });
}

/** The window is up on the demo repository, at the QA window size. */
export async function ready(page: Page): Promise<void> {
  await page.setViewportSize({ width: 1400, height: 900 });
  focusMain();
  // Park the real X pointer too (see `xPointer`): GPUI also sees it.
  execFileSync("xdotool", ["mousemove", String(140 * scale), String(880 * scale)]);
  await expect(
    page.getByRole("button", { name: "Command palette" }),
  ).toBeVisible({ timeout: 60_000 });
  await expect(row(page, "feat/empty-event-message")).toBeVisible({
    timeout: 60_000,
  });
}

/**
 * Gives the repo window the X input focus explicitly. Under Xvfb there is
 * no window manager, so focus follows the pointer (PointerRoot): moving
 * the real pointer into a popup menu would take focus from the repo
 * window, and menus close when it loses focus. With the focus set on the
 * window itself, the pointer can enter a menu and click its items.
 */
export function focusMain(): void {
  const ids = execFileSync("xdotool", ["search", "--onlyvisible", "--name", "^Treq - "])
    .toString()
    .trim()
    .split("\n");
  if (ids[0]) execFileSync("xdotool", ["windowfocus", "--sync", ids[0]]);
}

/**
 * The open popup (menu or popover) in the repo window's logical pixels:
 * the largest visible window other than the root and the repo window.
 */
export function popupBox(): { x: number; y: number; w: number; h: number } | null {
  const ids = execFileSync("xdotool", ["search", "--onlyvisible", "--class", ""])
    .toString()
    .trim()
    .split("\n");
  let best: { x: number; y: number; w: number; h: number } | null = null;
  for (const id of ids) {
    const g = execFileSync("xdotool", ["getwindowgeometry", "--shell", id]).toString();
    const v = Object.fromEntries(g.trim().split("\n").map((l) => l.split("=")));
    const x = Number(v.X) / scale;
    const y = Number(v.Y) / scale;
    const w = Number(v.WIDTH) / scale;
    const h = Number(v.HEIGHT) / scale;
    if (w < 20 || h < 20 || (x === 0 && y === 0 && w >= 1400)) continue;
    if (!best || w * h > best.w * best.h) best = { x, y, w, h };
  }
  return best;
}

/** A box in the repo window's logical pixels, as `popupBox` returns. */
type Box = { x: number; y: number; w: number; h: number };

/** The X screen's raw RGB pixels in `box`, popup windows included. */
export function screenPixels(box: Box): Buffer {
  const [x, y, w, h] = [box.x, box.y, box.w, box.h].map((v) => Math.round(v * scale));
  return execFileSync("import", ["-window", "root", "-crop", `${w}x${h}+${x}+${y}`, "rgb:-"]);
}

/**
 * Waits until the X screen in `box` differs from `from` and holds still
 * from one grab to the next: for what a popup window (a toast, a menu)
 * draws, which Playwright does not reach. Returns the new pixels.
 */
export async function screenChanged(box: Box, from: Buffer): Promise<Buffer> {
  let last = from;
  await expect
    .poll(
      () => {
        const now = screenPixels(box);
        const still = !now.equals(from) && now.equals(last);
        last = now;
        return still;
      },
      { message: "the popup redraws and holds still", intervals: [100] },
    )
    .toBe(true);
  return last;
}

type Clip = { x: number; y: number; width: number; height: number };

/** The terminal pane under the tabs, in the 1400x900 window. */
export const TERMINAL: Clip = { x: 280, y: 603, width: 1120, height: 297 };

/**
 * Waits until the window's pixels in `clip` hold still from one screenshot
 * to the next, and differ from `before` when given: for what the app draws
 * but does not expose as an element, such as terminal output. Returns the
 * last screenshot.
 */
export async function drawn(page: Page, clip: Clip, before?: Buffer): Promise<Buffer> {
  let last: Buffer | undefined;
  let now: Buffer = Buffer.alloc(0);
  await expect
    .poll(
      async () => {
        const previous = last;
        now = await page.screenshot({ clip });
        last = now;
        return !!previous && now.equals(previous) && !before?.equals(now);
      },
      { message: "the window draws and holds still", intervals: [100] },
    )
    .toBe(true);
  return now;
}

/**
 * Polls `probe` until it holds or `ms` passes, and says whether it held: for
 * the chaos runs, which record a state that never comes as a finding rather
 * than fail on it.
 */
export async function eventually(
  probe: () => Promise<boolean>,
  ms: number,
  interval = 2000,
): Promise<boolean> {
  const deadline = Date.now() + ms;
  let held = false;
  await expect
    .poll(
      async () => {
        held = await probe();
        return held || Date.now() >= deadline;
      },
      { intervals: [interval], timeout: ms + 60_000 },
    )
    .toBe(true);
  return held;
}

/**
 * The demo repository's home view. Only the selected repository's home row
 * has an accessible name (its branch, "main").
 */
export async function home(page: Page): Promise<void> {
  await ready(page);
  await page.keyboard.press("Escape");
  // The on-screen repository's home row is named by its branch; with
  // several repositories it heads that repository's group.
  await row(page, "main").first().click();
  await expect(page.getByRole("tab", { name: "Code" })).toBeVisible();
  await loaded(page);
}

export async function openWorkspace(page: Page, branch: string): Promise<void> {
  await ready(page);
  await row(page, branch).click();
  await expect(
    page.getByRole("button", { name: branch, exact: true, selected: true }),
  ).toBeVisible();
  await loaded(page);
}

export async function openTab(page: Page, name: string): Promise<void> {
  await page.getByRole("tab", { name }).click();
  await expect(page.getByRole("tab", { name, selected: true })).toBeVisible();
  await loaded(page);
}

/**
 * Waits for the screen's first reads to land. While a read runs, a screen
 * shows a progress indicator (`ui::Loading`, or "Loading workspace" over a
 * tab's stand-in), from the first frame on, even before it fades in.
 */
export async function loaded(page: Page): Promise<void> {
  await expect(page.getByRole("progressbar")).toHaveCount(0);
}

/** Opens the palette (Ctrl+K), drawn over the repo window. */
export async function palette(page: Page): Promise<void> {
  await page.keyboard.press(`${mod}+k`);
  await expect(page.getByRole("textbox", { name: "Type a command or search…" })).toBeVisible();
}

/** Settles async reads (PR polls, diffs) before a shot. */
export function settle(page: Page, ms = 1200): Promise<void> {
  return page.waitForTimeout(ms);
}

/**
 * Moves (and with `click`, clicks that button) the real X pointer at a point given in
 * the repo window's logical pixels. Menus, popovers and tooltips are popup
 * windows `page.mouse` does not reach, so hovering or clicking inside one
 * goes through `xdotool`. Assumes the repo window sits at the screen's
 * origin, as it does under Xvfb with no window manager. The app handles the
 * event on its own time: the caller waits for what it changes.
 */
export function xPointer(x: number, y: number, click: false | 1 | 3 = false): void {
  const args = ["mousemove", String(Math.round(x * scale)), String(Math.round(y * scale))];
  if (click) args.push("click", String(click));
  execFileSync("xdotool", args);
}

/**
 * Closes a popover (Checks, Details, the checks badge) or a menu. Popovers
 * draw in the repo window and close on Escape; menus are popup windows of
 * their own that only a real click outside dismisses (`page.mouse` reaches
 * the repo window only), on an empty part of the header row.
 */
export async function dismissPopover(page: Page): Promise<void> {
  await page.keyboard.press("Escape");
  const open = namedWindows().length;
  if (open > 1) {
    focusMain();
    xPointer(760, 40, 1);
    await expect.poll(() => namedWindows().length, { message: "the menu closes" }).toBeLessThan(open);
  }
}

/** The visible X windows with a name. */
function namedWindows(): string[] {
  return execFileSync("xdotool", ["search", "--onlyvisible", "--name", "."])
    .toString()
    .trim()
    .split("\n")
    .filter(Boolean);
}


/**
 * A .catch handler for a chaos action that may miss: random input moves the
 * screen, so its target can close, move or be covered before Playwright
 * reaches it. The miss goes to the run's notes and stands in as null; the
 * run's liveness checks catch real breakage.
 */
export function noteMiss(note: (what: string) => void, action: string): (err: unknown) => null {
  return (err) => {
    note(`${action} missed: ${err instanceof Error ? err.message.split("\n")[0] : "non-Error rejection"}`);
    return null;
  };
}
