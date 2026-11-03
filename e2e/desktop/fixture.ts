import { execFileSync } from "node:child_process";
import { existsSync, readFileSync } from "node:fs";
import { expect, type BrowserContext, type Page } from "@playwright/test";
import { fixtureFile } from "./env.ts";

/** What `launch.ts` set up for the running app. */
export interface Fixture {
  root: string;
  home: string;
  /** The git repository the app opened. */
  repo: string;
  /** One line per start of the fake `claude` agent: its arguments. */
  agentLog: string;
}

export function fixture(): Fixture {
  return JSON.parse(readFileSync(fixtureFile, "utf8")) as Fixture;
}

/** The fake agent's recorded starts. */
export function agentCalls(): string[] {
  const { agentLog } = fixture();
  if (!existsSync(agentLog)) return [];
  return readFileSync(agentLog, "utf8").split("\n").filter(Boolean);
}

/** `git` in the fixture repository (jj keeps it colocated). */
export function git(...args: string[]): string {
  const { repo, home } = fixture();
  return execFileSync("git", args, {
    cwd: repo,
    env: { ...process.env, HOME: home },
    encoding: "utf8",
  });
}

/** Every commit subject reachable from any ref or HEAD. */
export function commitSubjects(): string[] {
  return git("log", "--all", "--format=%s").split("\n").filter(Boolean);
}

/** The platform's command key, as the app's shortcuts read it. */
export const mod = process.platform === "darwin" ? "Meta" : "Control";

/**
 * A page for another app window once it shows (a second repository
 * window): the app hands a new page the active window (see
 * `gpui_shell::e2e`). Dialogs draw in the repo window itself, in `page`.
 */
export async function windowPage(context: BrowserContext): Promise<Page> {
  const deadline = Date.now() + 10_000;
  for (;;) {
    try {
      return await context.newPage();
    } catch (error) {
      // The window is not open, or not focused, yet.
      if (Date.now() > deadline) throw error;
      await new Promise((resolve) => setTimeout(resolve, 200));
    }
  }
}

/** Waits for the repo window to show its sidebar and home row. */
export async function appReady(page: Page): Promise<void> {
  await expect(
    page.getByRole("button", { name: "Command palette" }),
  ).toBeVisible({
    timeout: 30_000,
  });
  await expect(homeRow(page)).toBeVisible({ timeout: 30_000 });
}

/** The sidebar's home row, named by the home branch. */
export function homeRow(page: Page) {
  return page.getByRole("button", { name: "main", exact: true });
}

/** Back on the home view, as a click on the home row does. */
export async function goHome(page: Page): Promise<void> {
  await appReady(page);
  await homeRow(page).click();
  await expect(page.getByRole("tab", { name: "Code" })).toBeVisible();
}
