// Tuning the review prompt: Settings → Code Review → Tune with agent starts
// the tuning agent, its proposed edits (recorded here with the same
// `treq agent-review add` the agent runs) show on the review cards under the
// prompt, and Apply writes one into the prompt.
import { execFileSync } from "node:child_process";
import { existsSync, readdirSync, readFileSync, statSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { expect, test, type Page } from "@playwright/test";
import { appBinary } from "../env.ts";
import { capture } from "./capture.ts";
import { ready, settle } from "./helpers.ts";

test.describe.configure({ mode: "serial" });

/** The running QA app's fixture root. */
function fixtureRoot(): string {
  return readdirSync(tmpdir())
    .filter((d) => d.startsWith("treq-qa-"))
    .map((d) => join(tmpdir(), d))
    .filter((d) => {
      try {
        return statSync(join(d, "qa-extra.json")).isFile();
      } catch {
        return false;
      }
    })
    .sort((a, b) => statSync(b).mtimeMs - statSync(a).mtimeMs)[0];
}

function repoDir(): string {
  return JSON.parse(readFileSync(join(fixtureRoot(), "qa-extra.json"), "utf8")).repo;
}

/** Records a proposed edit the way the tuning agent does. */
function propose(line: number, comment: string, suggestion: string): void {
  const root = fixtureRoot();
  execFileSync(
    appBinary,
    [
      "agent-review", "add",
      "--target-type", "review_prompt", "--target-id", "repo",
      "--file", "body.md", "--start-line", String(line), "--end-line", String(line),
      "--comment", comment, "--suggestion", suggestion,
    ],
    {
      cwd: repoDir(),
      env: { ...process.env, HOME: join(root, "home"), PATH: `${join(root, "bin")}:${process.env.PATH}` },
      stdio: "pipe",
    },
  );
}

async function codeReviewSettings(page: Page): Promise<void> {
  await ready(page);
  const close = page.getByRole("button", { name: "Close", exact: true });
  if ((await close.count()) === 0) {
    await page.getByRole("button", { name: "Settings", exact: true }).click();
    await expect(close).toBeVisible();
  }
  await page.getByRole("tab", { name: "Repository" }).click();
  await page.mouse.move(950, 500);
  const tune = page.getByRole("button", { name: "Tune with agent" });
  for (let i = 0; i < 8 && !(await tune.isVisible()); i++) {
    await page.mouse.wheel(0, 400);
    await settle(page, 500);
  }
  await expect(tune).toBeVisible();
  await settle(page, 600);
}

test("tune review prompt: start, review edits, apply", async ({ page }) => {
  await codeReviewSettings(page);
  await capture(page, {
    name: "24-tune-01-settings",
    title: "Settings → Code Review before tuning",
    expectations: [
      "Tune with agent sits under Custom Review Prompt, an outline button",
      "Its help text says the proposed edits show below for review",
    ],
  });

  await page.getByRole("button", { name: "Tune with agent" }).click();
  await settle(page, 2500);
  const snapshot = join(repoDir(), ".treq", "review-prompt", "body.md");
  expect(existsSync(snapshot)).toBe(true);
  const firstLine = readFileSync(snapshot, "utf8").split("\n")[0];
  await capture(page, {
    name: "24-tune-02-agent",
    title: "Tune with agent: the tuning agent's terminal",
    expectations: ["A workspace page with an agent terminal named Tune review prompt"],
  });

  propose(1, "You resolve most correctness comments; lead with them.", `${firstLine} Lead with correctness bugs.`);
  propose(3, "You dismissed 6 of 7 naming comments in the last 30 days.", "Do not comment on naming or style.");
  await codeReviewSettings(page);
  const apply = page.getByRole("button", { name: "Apply to prompt" });
  await expect(apply.first()).toBeVisible({ timeout: 20_000 });
  await expect(apply).toHaveCount(2);
  await apply.first().scrollIntoViewIfNeeded();
  await settle(page, 600);
  await capture(page, {
    name: "24-tune-03-edits",
    title: "Settings: the agent's proposed edits on the review cards",
    expectations: [
      "Proposed edits (2) under the prompt, each a Local agent card with its line",
      "Each card shows the reason and the suggestion as a red/green diff",
      "Apply to prompt, Resolve and Delete buttons on each card",
    ],
  });

  await apply.first().click();
  await expect(apply).toHaveCount(1);
  const prompt = page.getByRole("textbox", { name: "Leave empty to use the built-in review prompt" });
  await expect(prompt).toHaveValue(/Lead with correctness bugs\./);
  await settle(page, 800);
  await capture(page, {
    name: "24-tune-04-applied",
    title: "Settings: one edit applied",
    expectations: [
      "The applied edit is gone; Proposed edits (1) remains",
      "The prompt field now contains the applied text",
    ],
  });
  await page.getByRole("button", { name: "Delete" }).first().click();
  await expect(apply).toHaveCount(0);
});
