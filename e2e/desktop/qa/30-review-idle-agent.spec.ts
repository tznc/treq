// Finishing a Changes review with "Use an idle agent" sends the whole
// templated review to the workspace's idle agent as one message. The fake
// agent runs in paste mode (qa/fake-paste-agent.mjs): it asks for bracketed
// paste and prints each message it receives with its line count.
import { readdirSync, readFileSync, statSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { expect, test, type Page } from "@playwright/test";
import { capture } from "./capture.ts";
import { openTab, openWorkspace, row, settle } from "./helpers.ts";

const ALERTING = "feat/alerting-logs";

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

async function comment(page: Page, y: number, text: string): Promise<void> {
  await page.mouse.click(600, y);
  await page.keyboard.press("ArrowLeft");
  const input = page.getByRole("textbox", { name: "Add a comment…" });
  await expect(input).toBeVisible();
  await input.fill(text);
  await page.getByRole("button", { name: "Add comment", exact: true }).click();
  await expect(input).toHaveCount(0);
  await settle(page, 600);
}

test("review: finish with an idle agent sends the whole review", async ({ page }) => {
  writeFileSync(join(fixtureRoot(), "paste-agent"), "");
  await openWorkspace(page, ALERTING);
  await row(page, ALERTING).hover();
  await row(page, ALERTING).getByRole("button", { name: "Start agent" }).click();
  // Quiet output long enough for the agent to count as idle.
  await settle(page, 4000);
  await openTab(page, "Changes");
  await settle(page, 1500);

  await comment(page, 237, "First comment: rename level");
  await comment(page, 333, "Second comment: add a test");

  await page.getByRole("button", { name: "Finish review" }).click();
  await settle(page, 600);
  // The popover's summary and checkbox are not observable, so by place in
  // the 1400x900 window.
  await page.mouse.click(1220, 245);
  await page.keyboard.type("Overall: looks close");
  await page.mouse.click(1088, 311);
  await settle(page, 400);
  await capture(page, {
    name: "30-review-idle-01-finish-popover",
    title: "Finish review with 'Use an idle agent' ticked",
    expectations: [
      "The Finish review popover lists 2 comments",
      "'Send to idle agent if one is available' is checked",
    ],
  });

  await page.getByRole("button", { name: "Edit", exact: true }).click();
  await settle(page, 2500);
  const received = readFileSync(join(fixtureRoot(), "paste-agent.log"), "utf8")
    .trim()
    .split("\n")
    .map((l) => JSON.parse(l) as string);
  expect(received).toHaveLength(1);
  expect(received[0]).toContain("First comment: rename level");
  expect(received[0]).toContain("Second comment: add a test");
  expect(received[0]).toContain("Overall: looks close");
  await page.getByRole("button", { name: "Maximize" }).click();
  await settle(page, 800);
  await page.mouse.move(800, 400);
  await page.mouse.wheel(0, -3000);
  await settle(page, 600);
  await capture(page, {
    name: "30-review-idle-02-agent-received",
    title: "The idle agent receives the review as one message",
    expectations: [
      "The agent column shows exactly one 'Message 1 (N lines)' block, N > 1",
      "That block holds the review template and both comments",
      "No 'Message 2' block: the review was not split per line",
    ],
  });
});
