// Navigation the QA specs share. Every spec starts from `ready`, so one
// area can run alone (`--grep`).
import { execFileSync } from "node:child_process";
import { expect, type BrowserContext, type Locator, type Page } from "@playwright/test";
import { dialogPage, mod } from "../fixture.ts";

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
    const [x, y, w, h] = ["X", "Y", "WIDTH", "HEIGHT"].map((k) => Number(v[k]) / scale);
    if (w < 20 || h < 20 || (x === 0 && y === 0 && w >= 1400)) continue;
    if (!best || w * h > best.w * best.h) best = { x, y, w, h };
  }
  return best;
}

/**
 * The demo repository's home view. Only the selected repository's home row
 * has an accessible name (its branch), so event-bus's row, the first under
 * the palette button, is clicked by place.
 */
export async function home(page: Page): Promise<void> {
  await ready(page);
  await page.keyboard.press("Escape");
  const palette = await page.getByRole("button", { name: "Command palette" }).boundingBox();
  if (!palette) throw new Error("no palette button");
  await page.mouse.click(100, palette.y + palette.height + 23);
  await expect(page.getByRole("tab", { name: "Code" })).toBeVisible();
  await page.waitForTimeout(800);
}

export async function openWorkspace(page: Page, branch: string): Promise<void> {
  await ready(page);
  await row(page, branch).click();
  await expect(
    page.getByRole("button", { name: branch, exact: true, selected: true }),
  ).toBeVisible();
  await page.waitForTimeout(800);
}

export async function openTab(page: Page, name: string): Promise<void> {
  await page.getByRole("tab", { name }).click();
  await expect(page.getByRole("tab", { name, selected: true })).toBeVisible();
  await page.waitForTimeout(1500);
}

/** The palette (Ctrl+K), in its dialog window. */
export async function palette(page: Page, context: BrowserContext): Promise<Page> {
  await page.keyboard.press(`${mod}+k`);
  return dialogPage(context);
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
 * origin, as it does under Xvfb with no window manager.
 */
export async function xPointer(
  page: Page,
  x: number,
  y: number,
  click: false | 1 | 3 = false,
): Promise<void> {
  const args = ["mousemove", String(Math.round(x * scale)), String(Math.round(y * scale))];
  if (click) args.push("click", String(click));
  execFileSync("xdotool", args);
  await page.waitForTimeout(600);
}

/**
 * The page of the dialog window the palette and the other overlays open
 * in. The app reuses one dialog window, and a page holds its window for
 * the test's context, so a second overlay in the same test reuses the
 * page the first one adopted.
 */
export async function overlay(page: Page, context: BrowserContext): Promise<Page> {
  const held = context.pages().find((p) => p !== page);
  if (held) return held;
  return dialogPage(context);
}

/**
 * Closes a popover (Checks, Details, the checks badge, menus). Popovers
 * ignore Escape and put a full-window backdrop window over the repo
 * window that only a real click dismisses (`page.mouse` reaches the repo
 * window only). Without a compositor that backdrop paints black. The click
 * lands on an empty part of the header row.
 */
export async function dismissPopover(page: Page): Promise<void> {
  await page.keyboard.press("Escape");
  const visible = execFileSync("xdotool", ["search", "--onlyvisible", "--name", "."])
    .toString()
    .trim()
    .split("\n")
    .filter(Boolean);
  if (visible.length > 1) {
    focusMain();
    await xPointer(page, 760, 40, 1);
  }
  await page.waitForTimeout(500);
}

/**
 * The page a dialog renders in: the repo window when it draws the dialog
 * itself, else the shared dialog window.
 */
export async function dialogIn(
  page: Page,
  context: BrowserContext,
  find: (p: Page) => Locator,
): Promise<Page> {
  try {
    await expect(find(page)).toBeVisible({ timeout: 4000 });
    return page;
  } catch {
    const other = await overlay(page, context);
    await expect(find(other)).toBeVisible();
    return other;
  }
}
