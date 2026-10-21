// The sidebar's Open Workspaces dialog: pasted branch names and GitHub
// links checked line by line, the open pull requests beside them (the
// user's own first, a search across everyone's), and opening several
// workspaces at once.
import { expect, test } from "@playwright/test";
import { capture } from "./capture.ts";
import { home, row, settle } from "./helpers.ts";

test.describe.configure({ mode: "serial" });

test("open-workspaces: paste, pick pull requests and open", async ({ page }) => {
  await home(page);
  const open = page.getByRole("button", { name: "Open workspaces" });
  await expect(open).toBeVisible();
  await capture(page, {
    name: "13-open-workspaces-sidebar-button",
    title: "Open button beside WORKSPACES",
    clip: { x: 0, y: 0, width: 300, height: 360 },
    expectations: ["A small '+ Open' button sits right-aligned on the WORKSPACES heading row"],
  });

  await open.click();
  await expect(page.getByRole("heading", { name: "Open Workspaces" })).toBeVisible();
  await expect(page.getByRole("heading", { name: "Your pull requests" })).toBeVisible();
  await settle(page, 800);
  await capture(page, {
    name: "13-open-workspaces-empty",
    title: "Open Workspaces dialog, nothing pasted yet",
    expectations: [
      "Left: 'Branches or links' textarea with placeholder, 'Nothing to open yet.', Cancel and a disabled Open button",
      "Right: 'Your pull requests' with a search box and octocat's open PRs as checkbox rows (#418, #412, #401)",
    ],
  });

  await page
    .getByLabel("Branches or links")
    .fill(
      [
        "chore/bump-deps",
        "feat/keyvalues-cache",
        "https://github.com/lorem-ipsum/event-bus/pull/421",
        "https://github.com/acme/other/pull/1",
        "main",
      ].join("\n"),
    );
  await expect(page.getByRole("status")).toHaveAccessibleName(
    "2 workspaces will be created, 1 already open",
    { timeout: 20_000 },
  );
  await page.getByLabel("Search pull requests").fill("runbook");
  await expect(page.getByRole("heading", { name: "Matching pull requests" })).toBeVisible();
  await page.getByRole("checkbox", { name: "#409 docs: add the ingest runbook" }).click();
  await expect(page.getByRole("status")).toHaveAccessibleName(
    "3 workspaces will be created, 1 already open",
  );
  await settle(page, 500);
  await capture(page, {
    name: "13-open-workspaces-checked",
    title: "Lines checked and a searched pull request picked",
    expectations: [
      "Status reads '3 workspaces will be created, 1 already open' and '2 lines can't be opened' in red",
      "Rows: chore/bump-deps new; feat/keyvalues-cache already open; fix/slack-retry PR #421 new; acme/other and main with red reasons; docs/ingest-runbook PR #409 new",
      "Right: 'Matching pull requests' lists only #409, checked; button reads 'Open 4 Workspaces'",
    ],
  });

  await page.getByRole("button", { name: "Open 4 Workspaces" }).click();
  await expect(page.getByRole("heading", { name: "Open Workspaces" })).toBeHidden({
    timeout: 30_000,
  });
  for (const branch of ["chore/bump-deps", "fix/slack-retry"]) {
    await expect(row(page, branch)).toBeVisible({ timeout: 20_000 });
  }
  await settle(page, 1500);
  await capture(page, {
    name: "13-open-workspaces-opened",
    title: "After opening: new rows in the sidebar",
    screen: true,
    expectations: [
      "Sidebar lists chore/bump-deps and fix/slack-retry as new workspaces",
      "chore/bump-deps (the first opened) is selected",
      "A 'Workspaces opened' toast reports what was created",
    ],
  });
});
