// Chaos on the Projects feature in the real app: targeted scenarios (double
// Save, rename to an existing name, save then rename fast, Home tab switches
// while reads are in flight, a home checkout deleted or committed to behind
// the app, a missing context path, a very long project name), then a seeded
// monkey over the project button and menu, its dialogs, Home's tabs, the
// repo headers, the cards, the palette, keys, zoom and window size. After
// every step the process must be alive, Playwright must answer and the app
// log (`<fixture>/app.log`, see harness.ts) must have no `panicked`.
// `QA_SEED` and `CHAOS_STEPS` vary the monkey; the seed and the trail print.
import { execFileSync } from "node:child_process";
import { readdirSync, readFileSync, renameSync, statSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { dirname, join } from "node:path";
import { expect, test, type Page } from "@playwright/test";
import { port } from "../env.ts";
import { capture } from "./capture.ts";
import { focusMain, home, popupBox, ready, row, settle, xPointer } from "./helpers.ts";

test.describe.configure({ mode: "serial" });

const STEPS = Number(process.env.CHAOS_STEPS ?? 150);
const seed0 = Number(process.env.QA_SEED ?? 2111);
let seed = seed0;
const trail: string[] = [];
let zoom = 0;

function random(): number {
  seed = (seed + 0x6d2b79f5) | 0;
  let t = seed;
  t = Math.imul(t ^ (t >>> 15), t | 1);
  t ^= t + Math.imul(t ^ (t >>> 7), t | 61);
  return ((t ^ (t >>> 14)) >>> 0) / 4294967296;
}
const pick = <T>(items: T[]): T => items[Math.floor(random() * items.length)];

/** This run's fixture: the parent of the app's HOME (see harness.ts). */
function fixtureRoot(): string {
  const env = readFileSync(`/proc/${appPid()}/environ`, "utf8").split("\0");
  const home = env.find((e) => e.startsWith("HOME="))?.slice(5);
  if (!home) throw new Error("app has no HOME");
  return dirname(home);
}

/** The app's pid: the process started with this run's GPUI_PLAYWRIGHT port. */
function appPid(): number {
  for (const d of readdirSync("/proc").filter((d) => /^\d+$/.test(d))) {
    try {
      const env = readFileSync(`/proc/${d}/environ`, "utf8").split("\0");
      if (env.includes(`GPUI_PLAYWRIGHT=${port}`)) return Number(d);
    } catch {}
  }
  throw new Error(`no app on port ${port}`);
}

let pid = 0;
let logPath = "";

function note(what: string) {
  trail.push(what);
}

/** Alive, answering and not panicked. */
async function alive(page: Page): Promise<void> {
  const what = trail.at(-1) ?? "start";
  try {
    process.kill(pid, 0);
  } catch {
    throw new Error(`app died after ${what}\nseed=${seed0}\n${trail.join("\n")}`);
  }
  await expect(page.getByRole("button", { name: "Command palette" }), `no answer after ${what}`)
    .toBeVisible({ timeout: 20_000 });
  const log = readFileSync(logPath, "utf8");
  const at = log.indexOf("panicked");
  expect(at, `panic after ${what} (seed ${seed0}):\n${log.slice(Math.max(0, at - 400), at + 600)}\n${trail.join("\n")}`).toBe(-1);
}

const projectButton = (page: Page) => page.getByRole("button", { name: /^Project: / });

/** Opens the project menu; returns its box. */
async function openMenu(page: Page) {
  focusMain();
  await projectButton(page).click();
  await settle(page, 700);
  return popupBox();
}

/** Clicks a menu row: offset from the top (>0) or from the bottom (<0). */
async function menuRow(page: Page, offset: number): Promise<boolean> {
  const menu = await openMenu(page);
  if (!menu) return false;
  const y = offset > 0 ? menu.y + offset : menu.y + menu.h + offset;
  await xPointer(page, menu.x + 60, y, 1);
  focusMain();
  await settle(page, 700);
  return true;
}

async function clearField(page: Page) {
  // One key press costs about a second on a loaded llvmpipe box.
  await page.keyboard.press("Control+a");
  await page.keyboard.press("Backspace");
}

async function saveAs(page: Page, name: string, enters = 1) {
  await menuRow(page, 18); // Save Project… (unsaved) / the project (saved)
  await clearField(page);
  await page.keyboard.type(name);
  for (let i = 0; i < enters; i++) await page.keyboard.press("Enter");
  await settle(page, 1500);
}

async function rename(page: Page, name: string) {
  await escAll(page);
  await menuRow(page, 61); // Rename Project… (saved menu: project, rename, instructions, new window)
  await clearField(page);
  await page.keyboard.type(name);
  await page.keyboard.press("Enter");
  await settle(page, 1200);
}

async function escAll(page: Page) {
  for (let i = 0; i < 3; i++) await page.keyboard.press("Escape");
  await settle(page, 300);
}

async function shot(page: Page, name: string, title: string, expectations: string[]) {
  await capture(page, { name: `21-projects-chaos-${name}`, title, expectations, rest: false });
}

test.beforeAll(() => {
  logPath = join(fixtureRoot(), "app.log");
  pid = appPid();
  console.log(`[projects-chaos] seed=${seed0} pid=${pid} log=${logPath}`);
});

test.afterAll(() => {
  console.log(`[projects-chaos] seed=${seed0} trail:\n${trail.join("\n")}`);
});

test("projects-chaos: targeted scenarios", async ({ page }) => {
  test.setTimeout(1_800_000);
  await ready(page);
  await settle(page, 2500);
  await home(page);

  note("double save (Enter Enter Enter)");
  await saveAs(page, "Chaos one", 3);
  await settle(page, 4000);
  await alive(page);
  await shot(page, "01-double-save", "After a triple Enter on Save project", [
    "One project 'Chaos one'; no duplicate in the menu, no stray dialog",
  ]);
  const menu = await openMenu(page);
  await capture(page, { name: "21-projects-chaos-02-menu", title: "Menu after double save", screen: true, rest: false, expectations: ["Exactly one '✓ Chaos one' row"] });
  if (!menu) throw new Error("no menu");
  await escAll(page);
  focusMain();
  await xPointer(page, 760, 40, 1);

  note("rename to the same name, then to empty, then to unicode");
  await rename(page, "Chaos one");
  await alive(page);
  await rename(page, "");
  await alive(page);
  await shot(page, "03-rename-empty", "Rename to an empty name", [
    "Empty name refused (dialog stays or name unchanged); never a blank project button",
  ]);
  await escAll(page);
  await rename(page, "Ünïcødé 🚀 项目 ‮evil");
  await alive(page);

  note("save then rename rapidly");
  await rename(page, "Fast A");
  await rename(page, "Fast B");
  await settle(page, 3000);
  await alive(page);
  await shot(page, "04-rapid-rename", "Two renames back to back", ["The button reads 'Fast B'"]);

  note("instructions with a missing context path");
  await escAll(page);
  await menuRow(page, 93); // Project Instructions…
  await page.keyboard.type("Instructions ".repeat(40));
  await page.keyboard.press("Tab");
  await page.keyboard.type("/does/not/exist/AGENTS.md\n../../nope.md");
  await shot(page, "05-instructions-missing", "Instructions dialog with a missing context path", [
    "A missing path is flagged or saved harmlessly; no crash",
  ]);
  const save = page.getByRole("button", { name: "Save", exact: true });
  if (await save.count()) {
    await save.click({ timeout: 3000 }).catch(() => {});
    await save.click({ timeout: 1000 }).catch(() => {});
  }
  await settle(page, 2000);
  await alive(page);
  await escAll(page);

  note("home tab switching while reads are in flight");
  await escAll(page);
  await page.getByRole("button", { name: "Home", exact: true }).click();
  for (let i = 0; i < 15; i++) {
    await page.getByRole("tab", { name: pick(["Code", "Changes", "Commits"]) }).click();
    await page.waitForTimeout(pick([0, 20, 80, 200]));
  }
  await settle(page, 3000);
  await alive(page);
  await shot(page, "06-tab-spam", "Home after 30 fast tab switches", [
    "The selected tab's content matches the tab; no stale columns from another tab",
  ]);

  note("home checkout deleted behind the app, then a commit in the other");
  const root = fixtureRoot();
  const gone = join(root, "docs-site");
  renameSync(gone, `${gone}.moved`);
  execFileSync("git", ["-C", join(root, "event-bus"), "-c", "user.name=QA", "-c", "user.email=qa@x",
    "commit", "--allow-empty", "-m", "chaos: behind the app"]);
  for (const tab of ["Code", "Changes", "Commits"]) {
    await page.getByRole("tab", { name: tab }).click();
    await settle(page, 2500);
    await alive(page);
    await shot(page, `07-deleted-${tab.toLowerCase()}`, `Home › ${tab} with docs-site deleted`, [
      "docs-site shows an error or empty state, event-bus still loads; 'chaos: behind the app' in Commits",
    ]);
  }
  const card = page.getByRole("button", { name: "Open docs-site" });
  if (await card.count()) {
    note("open the deleted repo's card");
    await card.click();
    await settle(page, 2000);
    await alive(page);
    await shot(page, "08-deleted-open", "Opening the deleted repository", ["An error state, no crash"]);
  }
  renameSync(`${gone}.moved`, gone);
  await settle(page, 2000);

  note("very long project name");
  await home(page);
  await rename(page, "A very long project name that keeps going ".repeat(12).slice(0, 500));
  await settle(page, 2000);
  await alive(page);
  await shot(page, "09-long-name", "Project button with a 500-char name", [
    "The button truncates with an ellipsis and leaves the palette button in place",
  ]);
  await rename(page, "Chaos one");
});

async function step(page: Page): Promise<string> {
  const kind = pick([
    "menu", "menu", "menu", "dialogText", "dialogText", "keys", "keys", "homeTab", "homeTab",
    "header", "card", "palette", "zoom", "resize",
  ]);
  switch (kind) {
    case "menu": {
      const offset = pick([18, 18, 61, 93, 0]);
      if (offset === 0) {
        await openMenu(page);
        focusMain();
        await xPointer(page, 760, 40, 1);
        return "menu open+dismiss";
      }
      await menuRow(page, offset);
      return `menu row ${offset}`;
    }
    case "dialogText": {
      const text = pick([
        "", "x", "名前 ✨ ñ", "a".repeat(500), "\u0000\u0007tab\tnl", "  spaced  ", "Chaos one", "🙂".repeat(60),
      ]);
      if (random() < 0.5) await clearField(page);
      await page.keyboard.type(text);
      const after = pick(["Enter", "Escape", "Enter Enter", "none"]);
      if (after !== "none") for (const k of after.split(" ")) await page.keyboard.press(k);
      return `type ${JSON.stringify(text.slice(0, 20))}(${text.length}) then ${after}`;
    }
    case "keys": {
      const keys = pick(["Escape", "Tab", "Enter", "Escape Escape Escape", "Enter Enter", "Shift+Tab", "Tab Enter"]);
      for (const k of keys.split(" ")) await page.keyboard.press(k);
      return `keys ${keys}`;
    }
    case "homeTab": {
      await page.keyboard.press("Escape");
      const homeBtn = page.getByRole("button", { name: "Home", exact: true });
      if (await homeBtn.count()) await homeBtn.first().click({ timeout: 3000 }).catch(() => {});
      const tab = pick(["Code", "Changes", "Commits"]);
      await page.getByRole("tab", { name: tab }).first().click({ timeout: 3000 }).catch(() => {});
      return `home tab ${tab}`;
    }
    case "header": {
      const rows = row(page, "main");
      const n = await rows.count();
      if (!n) return "header none";
      const i = Math.floor(random() * n);
      await rows.nth(i).click({ timeout: 3000 }).catch(() => {});
      return `header ${i}`;
    }
    case "card": {
      const cards = page.getByRole("button", { name: /^Open (event-bus|docs-site|lorem)/ });
      const n = await cards.count();
      if (!n) return "card none";
      const i = Math.floor(random() * n);
      await cards.nth(i).click({ timeout: 3000 }).catch(() => {});
      return `card ${i}`;
    }
    case "palette": {
      await page.keyboard.press("Control+k");
      await page.keyboard.type(pick(["project", "save", "zzz", ""]));
      const k = pick(["Escape", "Enter", "ArrowDown Enter"]);
      for (const key of k.split(" ")) await page.keyboard.press(key);
      return `palette ${k}`;
    }
    case "zoom": {
      const k = pick(["Control+=", "Control+-", "Control+0"]);
      zoom = k === "Control+0" ? 0 : zoom + (k === "Control+=" ? 1 : -1);
      await page.keyboard.press(k);
      return `zoom ${k}`;
    }
    default: {
      const size = pick([{ width: 320, height: 240 }, { width: 1900, height: 1180 }, { width: 1400, height: 900 }, { width: 500, height: 900 }]);
      await page.setViewportSize(size);
      return `resize ${size.width}x${size.height}`;
    }
  }
}

test("projects-chaos: seeded monkey", async ({ page }) => {
  test.setTimeout(3_600_000);
  await ready(page);
  await settle(page, 1500);
  for (let i = 0; i < STEPS; i++) {
    note(`#${i} ${await step(page)}`);
    await page.waitForTimeout(150);
    await alive(page);
    if (i % 25 === 24) {
      await capture(page, { name: `21-projects-chaos-monkey-${seed0}-${i + 1}`, title: `Monkey step ${i + 1}`, rest: false, expectations: ["Window drawn, nothing overflows"] });
    }
  }
  await page.keyboard.press("Control+0");
  await page.setViewportSize({ width: 1400, height: 900 });
  await escAll(page);
  await alive(page);
});
