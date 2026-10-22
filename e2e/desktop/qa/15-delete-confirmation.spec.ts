// The delete-workspace confirmation belongs to the repo window: it is
// transient for it, so the window manager (openbox here) keeps it above the
// app, and asking to delete again while it is open doesn't stack a second
// one. The question is answered No, so nothing is deleted.
import { execFileSync, spawn } from "node:child_process";
import { expect, test, type BrowserContext, type Page } from "@playwright/test";
import { capture } from "./capture.ts";
import { home, mod, overlay, settle } from "./helpers.ts";

test.describe.configure({ mode: "serial" });

const BRANCH = "feat/keyvalues-cache";

function windows(name: string): string[] {
  try {
    return execFileSync("xdotool", ["search", "--onlyvisible", "--name", name])
      .toString()
      .trim()
      .split("\n")
      .filter(Boolean);
  } catch {
    return [];
  }
}

/** Picks Delete Workspace for `BRANCH` from the palette. */
async function askDelete(page: Page, context: BrowserContext): Promise<void> {
  await page.keyboard.press(`${mod}+k`);
  const palette = await overlay(page, context);
  await palette.getByRole("textbox", { name: "Type a command or search..." }).fill("Delete Workspace");
  await palette.getByRole("option", { name: "Delete Workspace" }).click();
  // The picker draws in the repo window or in the shared dialog window.
  const find = (p: Page) => p.getByRole("textbox", { name: "Search workspaces to delete..." });
  let picker: Page | undefined;
  await expect
    .poll(async () => {
      picker = (await find(page).isVisible())
        ? page
        : (await find(palette).isVisible())
          ? palette
          : undefined;
      return !!picker;
    })
    .toBe(true);
  if (!picker) throw new Error("no delete picker");
  await find(picker).fill(BRANCH);
  await settle(page, 600);
  await picker.keyboard.press("Enter").catch(() => {});
  await page.waitForTimeout(2000);
}

test("delete-confirmation: one question, owned by the repo window", async ({ page, context }) => {
  await home(page);
  // Stacking and placement are the window manager's job, and Xvfb has none.
  const wm = spawn("openbox", [], { stdio: "ignore", detached: true });
  await page.waitForTimeout(1500);
  try {
    await askDelete(page, context);
    expect(windows("^Delete Workspace$")).toHaveLength(1);
    await askDelete(page, context);
    const questions = windows("^Delete Workspace$");
    // Each question names its owner window; none means a stray dialog the
    // window manager can bury under the app.
    const owners = questions.map((id) =>
      execFileSync("xprop", ["-id", id, "WM_TRANSIENT_FOR"]).toString().trim(),
    );
    console.log(`open delete questions: ${questions.length}\n${owners.join("\n")}`);
    // Raise the repo window, as clicking back into the app does.
    execFileSync("xdotool", ["windowraise", windows("^Treq")[0]]);
    await page.waitForTimeout(800);
    await capture(page, {
      name: "15-delete-confirmation-asked-twice",
      title: "Delete Workspace picked twice",
      screen: true,
      rest: false,
      expectations: [
        "A single Delete Workspace question, above the raised repo window",
      ],
    });
    expect(questions).toHaveLength(1);
    expect(owners[0]).toMatch(/window id # 0x/);
  } finally {
    for (const id of windows("^Delete Workspace$")) {
      execFileSync("xdotool", ["key", "--window", id, "Escape"]);
      await page.waitForTimeout(500);
    }
    if (wm.pid) process.kill(-wm.pid);
  }
  await expect(page.getByRole("button", { name: new RegExp(BRANCH) }).first()).toBeVisible();
});
