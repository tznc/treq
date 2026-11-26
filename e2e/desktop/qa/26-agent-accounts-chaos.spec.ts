// Chaos on agent account profiles in the real app: the Agent accounts
// section of Application settings (adding valid and hostile names,
// double-clicks, removing, switching defaults) and prompts routed with
// @mentions, mixed with Escape, zoom and screen switches. A seeded monkey
// picks the steps. After every step the app must be alive, answer
// Playwright and log no `panicked`, and every agent it started must have a
// config dir that is unset or a valid profile's. `SEED` and `CHAOS_STEPS`
// vary the run.
import { copyFileSync, existsSync, readFileSync, writeFileSync } from "node:fs";
import { join } from "node:path";
import { expect, test, type Page } from "@playwright/test";
import { errorMessage } from "../error-message.ts";
import { fixture, mod } from "../fixture.ts";
import { capture } from "./capture.ts";
import { openWorkspace, ready, settle } from "./helpers.ts";

const STEPS = Number(process.env.CHAOS_STEPS ?? 60);
let seed = Number(process.env.SEED ?? 26);
const seed0 = seed;
const SHORT = { timeout: 2500 };

/** mulberry32: the same seed replays the same run. */
function rand(): number {
  seed |= 0;
  seed = (seed + 0x6d2b79f5) | 0;
  let t = Math.imul(seed ^ (seed >>> 15), 1 | seed);
  t = (t + Math.imul(t ^ (t >>> 7), 61 | t)) ^ t;
  return ((t ^ (t >>> 14)) >>> 0) / 4294967296;
}
const pick = <T>(xs: readonly T[]): T => xs[Math.floor(rand() * xs.length)] as T;

const NAMES = [
  "work",
  "personal",
  "side_job-2",
  "a".repeat(32),
  "a".repeat(33),
  "Work",
  "wörk",
  "my work",
  "  padded  ",
  "../etc",
  "$(touch pwned)",
  "😀",
  "",
  "-",
];
const MENTIONS = ["@work ", "@personal ", "@nobody ", "@ ", "@work/deep ", "@😀 ", "", "", ""];
const TASKS = ["Fix the flaky test", "x", "Add a TTL\nwith a newline", "ünïcödé task", "y".repeat(400)];

const trail: string[] = [];
const findings: string[] = [];
let logStart = 0;
let shot = 0;

const root = () => fixture().root;
const appLog = () => {
  const path = join(root(), "app.log");
  return existsSync(path) ? readFileSync(path, "utf8") : "";
};
const envLog = () => join(root(), "claude-env.log");

/** The app is alive, answers Playwright, logged no panic, and started every agent on a valid profile. */
async function alive(page: Page, what: string): Promise<void> {
  const ctx = `after: ${what}\nseed ${seed0} trail: ${trail.slice(-20).join(" | ")}`;
  const log = appLog().slice(logStart);
  expect(log.includes("panicked"), `panic in app log\n${ctx}\n${log.slice(log.indexOf("panicked") - 400)}`).toBe(false);
  expect(log.includes("[harness] app exited"), `app exited\n${ctx}`).toBe(false);
  await expect(page.getByRole("button").first(), `Playwright gets no answer\n${ctx}`).toBeAttached({
    timeout: 10_000,
  });
  if (existsSync(envLog())) {
    for (const line of readFileSync(envLog(), "utf8").split("\n").filter(Boolean)) {
      const dir = line.replace(/^CLAUDE_CONFIG_DIR=/, "");
      expect(
        dir === "" || /\/agent-profiles\/claude\/[a-z0-9_-]{1,32}$/.test(dir),
        `agent started with config dir ${JSON.stringify(dir)}\n${ctx}`,
      ).toBe(true);
    }
  }
  expect(existsSync(join(root(), "pwned")) || existsSync(join(fixture().repo, "pwned")), `a name ran as a command\n${ctx}`).toBe(false);
}

/** Runs a step; a control that isn't there is a miss, not a failure. */
async function step(page: Page, what: string, run: () => Promise<void>): Promise<void> {
  trail.push(what);
  try {
    await run();
  } catch (err) {
    trail.push(`  miss: ${(errorMessage(err).split("\n")[0] ?? "").slice(0, 120)}`);
    await page.keyboard.press("Escape").catch(() => {});
  }
  await settle(page, 250);
  await alive(page, what);
}

async function inSettings(page: Page): Promise<boolean> {
  return (await page.getByRole("button", { name: "Back", exact: true }).count()) > 0;
}

async function openAccounts(page: Page): Promise<void> {
  if (!(await inSettings(page))) {
    await page.getByRole("button", { name: "Settings", exact: true }).click(SHORT);
  }
  await page.getByRole("tab", { name: "Application" }).click(SHORT);
  await page.getByRole("textbox", { name: "work" }).scrollIntoViewIfNeeded(SHORT);
}

async function chooseOption(page: Page, combobox: string): Promise<void> {
  await page.getByRole("combobox", { name: combobox, exact: true }).click(SHORT);
  for (let i = Math.floor(rand() * 3); i > 0; i--) await page.keyboard.press("ArrowDown");
  await page.keyboard.press(pick(["Enter", "Enter", "Escape"]));
}

const actions: Action[] = [
  [5, "add profile", async (page) => {
    await openAccounts(page);
    if (rand() < 0.3) await chooseOption(page, "Agent");
    const name = pick(NAMES);
    trail.push(`  name ${JSON.stringify(name)}`);
    await page.getByRole("textbox", { name: "work" }).fill(name, SHORT);
    const add = page.getByRole("button", { name: "Add", exact: true });
    if (rand() < 0.25) await add.dblclick(SHORT);
    else await add.click(SHORT);
  }],
  [2, "remove profile", async (page) => {
    await openAccounts(page);
    const remove = page.getByRole("button", { name: "Remove", exact: true });
    const n = await remove.count();
    if (n > 0) await remove.nth(Math.floor(rand() * n)).click(SHORT);
  }],
  [2, "switch default account", async (page) => {
    await openAccounts(page);
    await chooseOption(page, pick(["Default Claude account", "Default Codex account"]));
  }],
  [4, "prompt", async (page) => {
    if (await inSettings(page)) await page.getByRole("button", { name: "Back", exact: true }).click(SHORT);
    await openWorkspace(page, "feat/keyvalues-cache");
    await page.getByRole("button", { name: "Start an agent", exact: true }).click(SHORT);
    const text = `${pick(MENTIONS)}${pick(TASKS)}`;
    trail.push(`  prompt ${JSON.stringify(text.slice(0, 40))}`);
    await page.getByRole("textbox", { name: "Describe a task…" }).last().fill(text, SHORT);
    if (rand() < 0.8) await page.getByRole("button", { name: "Send", exact: true }).last().click(SHORT);
    else await page.keyboard.press("Escape");
    await settle(page, 1200);
  }],
  [1, "escape", async (page) => {
    for (let i = 0; i < 3; i++) await page.keyboard.press("Escape");
  }],
  [1, "zoom", async (page) => {
    await page.keyboard.press(`${mod}+${pick(["=", "-", "0"])}`);
  }],
  [1, "back", async (page) => {
    if (await inSettings(page)) await page.getByRole("button", { name: "Back", exact: true }).click(SHORT);
  }],
];

type Action = [number, string, (page: Page) => Promise<void>];

function nextAction(): Action {
  const total = actions.reduce((sum, [w]) => sum + w, 0);
  let r = rand() * total;
  for (const action of actions) {
    r -= action[0];
    if (r < 0) return action;
  }
  return actions[0] as Action;
}

test("agent accounts chaos", async ({ page }) => {
  test.setTimeout(STEPS * 20_000 + 60_000);
  await ready(page);
  logStart = appLog().length;
  // The QA agent records the config dir it starts with, then waits.
  const agent = join(root(), "bin", "claude");
  copyFileSync(agent, `${agent}.qa`);
  writeFileSync(
    agent,
    `#!/bin/sh\nprintf 'CLAUDE_CONFIG_DIR=%s\\n' "$CLAUDE_CONFIG_DIR" >> '${envLog()}'\nprintf 'account: %s\\n' "$CLAUDE_CONFIG_DIR"\nexec cat\n`,
  );
  try {
    for (let i = 0; i < STEPS; i++) {
      const [, name, run] = nextAction();
      await step(page, `${i} ${name}`, () => run(page));
      if (i % 20 === 19) {
        shot++;
        await capture(page, {
          name: `26-agent-accounts-chaos-s${seed0}-${String(shot).padStart(2, "0")}`,
          title: `Chaos seed ${seed0}, step ${i}`,
          expectations: ["The window draws normally: no blank, frozen or clipped screen"],
        });
      }
    }
  } finally {
    copyFileSync(`${agent}.qa`, agent);
    const started = existsSync(envLog()) ? readFileSync(envLog(), "utf8").trim().split("\n").filter(Boolean) : [];
    console.log(`[accounts-chaos seed=${seed0}] agents started: ${started.length}`);
    console.log(`[accounts-chaos seed=${seed0}] ${started.map((l) => l.replace(/^.*agent-profiles\//, "")).join(", ")}`);
    console.log(`[accounts-chaos seed=${seed0}] findings:\n${findings.join("\n") || "(none)"}`);
    console.log(`[accounts-chaos seed=${seed0}] trail:\n${trail.join("\n")}`);
  }
});
