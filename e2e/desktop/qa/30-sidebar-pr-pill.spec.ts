// Workspace tooltips show the PR's state as a GitHub-style pill: Open,
// Draft, Merged or Closed, and no pill without a PR.
import { rmSync, writeFileSync } from "node:fs";
import { join } from "node:path";
import { test } from "@playwright/test";
import { fixture } from "../fixture.ts";
import { capture } from "./capture.ts";
import { home, ready, row, settle } from "./helpers.ts";

const overrides = () => join(fixture().home, "fake-gh-prs.json");

test.describe.configure({ mode: "serial" });
test.afterAll(() => rmSync(overrides(), { force: true }));

test("sidebar-pr-pill: workspace tooltips show the PR state pill", async ({ page }) => {
  test.setTimeout(180_000);
  await ready(page);
  // GitHub reports one PR closed and one a draft; the next polls read it.
  writeFileSync(
    overrides(),
    JSON.stringify({
      "feat/keyvalues-cache": { state: "CLOSED" },
      "feat/event-ingest": { isDraft: true },
    }),
  );
  await home(page);
  await settle(page, 25_000);

  const cases: [string, string, string][] = [
    ["feat/empty-event-message", "open", "A green Open pill with the pull request icon"],
    ["feat/event-ingest", "draft", "A grey Draft pill with the draft PR icon"],
    ["feat/alerting-logs", "merged", "A purple Merged pill with the merge icon"],
    ["feat/keyvalues-cache", "closed", "A red Closed pill with the closed PR icon"],
    ["main", "no-pr", "No pill: only the branch line"],
  ];
  for (const [branch, what, pill] of cases) {
    await row(page, branch).first().hover();
    await settle(page, 1500);
    await capture(page, {
      name: `30-sidebar-pr-pill-${what}`,
      title: `Tooltip on ${branch}`,
      screen: true,
      rest: false,
      expectations: [`The tooltip names ${branch}`, pill, "The pill fits inside the tooltip, unclipped"],
    });
    await page.mouse.move(700, 450);
    await settle(page, 500);
  }
});
