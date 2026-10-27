// The New Workspaces dialog (sidebar New, Ctrl+N, Stack): titles, new
// and existing branch names and GitHub links checked line by line (listed in a popover from the summary), the
// integrations beside them (GitHub's open pull requests, the user's own
// first, a search across everyone's; Linear's task list from the fake
// Linear), and opening several workspaces at once.
import { createHash } from "node:crypto";
import { join } from "node:path";
import { DatabaseSync } from "node:sqlite";
import { expect, test } from "@playwright/test";
import { fixture, mod } from "../fixture.ts";
import { capture } from "./capture.ts";
import { home, row, settle } from "./helpers.ts";

/** Gives the QA repository a Linear API key (`Database::make_repo_key`), so
 * it reaches the fake Linear; `null` takes it away. */
function linearKey(value: string | null): void {
  const { home, repo } = fixture();
  const hash = createHash("sha256").update(repo).digest("hex").slice(0, 16);
  const key = `repo_${hash}_linear_api_key`;
  const db = new DatabaseSync(join(home, ".local/share/com.treq/treq.db"));
  if (value === null) db.prepare("DELETE FROM settings WHERE key = ?").run(key);
  else
    db.prepare(
      "INSERT OR REPLACE INTO settings (key, value) VALUES (?, ?)",
    ).run(key, value);
  db.close();
}

test.describe.configure({ mode: "serial" });
test.afterAll(() => linearKey(null));

test("open-workspaces: type, pick pull requests and issues, and create", async ({
  page,
}) => {
  await home(page);
  const open = page.getByRole("button", { name: "New workspaces" });
  await expect(open).toBeVisible();
  await capture(page, {
    name: "13-open-workspaces-sidebar-button",
    title: "New button beside WORKSPACES",
    clip: { x: 0, y: 0, width: 300, height: 360 },
    expectations: [
      "A small 'New' button with a git-branch-plus icon sits right-aligned on the WORKSPACES heading row",
    ],
  });

  linearKey("lin_api_qa");
  await open.click();
  await expect(
    page.getByRole("heading", { name: "New Workspaces" }),
  ).toBeVisible();
  await expect(
    page.getByRole("heading", { name: "Your pull requests" }),
  ).toBeVisible();
  await settle(page, 800);
  await capture(page, {
    name: "13-open-workspaces-empty",
    title: "New Workspaces dialog, nothing typed yet",
    expectations: [
      "Left: a 'Workspaces' textarea with placeholder, 'Stack on' select reading main, an 'Advanced' toggle, 'Nothing to create yet.', Cancel and a disabled Create button",
      "Right: GitHub / Linear tabs (GitHub selected), 'Your pull requests' with a search box and octocat's open PRs as checkbox rows (#418, #412, #401)",
    ],
  });

  await page.getByRole("checkbox", { name: /^#418/ }).hover();
  await settle(page, 500);
  await capture(page, {
    name: "13-open-workspaces-row-hover",
    title: "New Workspaces dialog: a hovered pull request row",
    rest: false,
    expectations: [
      "Hovered #418 row fills with the same grey tint as a hovered sidebar row",
      "The other pull request rows stay unfilled",
    ],
  });

  await page
    .getByLabel("Workspaces", { exact: true })
    .fill(
      [
        "Retry flaky webhook tests",
        "chore/bump-deps",
        "feat/keyvalues-cache",
        "https://github.com/lorem-ipsum/event-bus/pull/421",
        "https://github.com/acme/other/pull/1",
        "main",
      ].join("\n"),
    );
  await expect(page.getByRole("status")).toHaveAccessibleName(
    "3 workspaces will be created, 1 already open",
    { timeout: 20_000 },
  );
  await page.getByLabel("Search pull requests").fill("runbook");
  await expect(
    page.getByRole("heading", { name: "Matching pull requests" }),
  ).toBeVisible();
  await page
    .getByRole("checkbox", { name: "#409 docs: add the ingest runbook" })
    .click();
  await expect(page.getByRole("status")).toHaveAccessibleName(
    "4 workspaces will be created, 1 already open",
  );
  await page.getByRole("button", { name: "Advanced" }).click();
  await expect(page.getByLabel("Sparse paths (one per line)")).toBeVisible();
  await settle(page, 500);
  await capture(page, {
    name: "13-open-workspaces-checked",
    title: "Lines checked and a searched pull request picked",
    expectations: [
      "Status reads '4 workspaces will be created, 1 already open' with a chevron, and '2 lines can't be opened' in red",
      "Advanced is open: 'Sparse paths' and 'Symlink from home repo' textareas side by side under Stack on",
      "Right: 'Matching pull requests' lists only #409, checked; the GitHub tab shows a '1' badge; button reads 'Create 5 Workspaces'",
    ],
  });

  await page.getByRole("status").click();
  await expect(
    page.getByRole("list", { name: "Workspaces to open" }),
  ).toBeVisible();
  await settle(page, 500);
  await capture(page, {
    name: "13-open-workspaces-popover",
    title: "The summary's popover lists each line",
    expectations: [
      "A popover by the summary lists: treq/retry-flaky-webhook-tests new branch; chore/bump-deps new; feat/keyvalues-cache already open; fix/slack-retry PR #421 new; acme/other and main with red reasons; docs/ingest-runbook PR #409 new",
      "The popover opens upward from the summary on the button row, with a border and shadow",
    ],
  });
  await page.keyboard.press("Escape");
  await expect(
    page.getByRole("list", { name: "Workspaces to open" }),
  ).toBeHidden();
  await expect(
    page.getByRole("heading", { name: "New Workspaces" }),
  ).toBeVisible();

  await page.getByRole("tab", { name: "Linear" }).click();
  await expect(page.getByRole("heading", { name: "Your issues" })).toBeVisible({
    timeout: 20_000,
  });
  await page
    .getByRole("checkbox", { name: "ENG-142 Retry webhook deliveries" })
    .click();
  await expect(page.getByRole("status")).toHaveAccessibleName(
    "5 workspaces will be created, 1 already open",
  );
  await settle(page, 500);
  await capture(page, {
    name: "13-open-workspaces-linear",
    title: "Linear tab: the user's task list",
    expectations: [
      "Linear tab selected with a '1' badge; GitHub tab keeps its '1' badge",
      "'Your issues' lists ENG-143, ENG-142, ENG-139 and ENG-131 (not the done ENG-128 or hubot's ENG-125), with state, priority and project badges",
      "ENG-142 is checked; status reads '5 workspaces will be created, 1 already open'; button reads 'Create 6 Workspaces'",
    ],
  });

  await page.getByRole("button", { name: "Create 6 Workspaces" }).click();
  await expect(
    page.getByRole("heading", { name: "New Workspaces" }),
  ).toBeHidden({
    timeout: 30_000,
  });
  for (const branch of [
    "treq/retry-flaky-webhook-tests",
    "chore/bump-deps",
    "fix/slack-retry",
  ]) {
    await expect(row(page, branch)).toBeVisible({ timeout: 20_000 });
  }
  await settle(page, 1500);
  await capture(page, {
    name: "13-open-workspaces-opened",
    title: "After opening: new rows in the sidebar",
    screen: true,
    expectations: [
      "Sidebar lists treq/retry-flaky-webhook-tests, chore/bump-deps, fix/slack-retry and the ENG-142 branch as new workspaces",
      "treq/retry-flaky-webhook-tests (the first line) is selected",
      "A one-line toast (\"Created N workspaces\") reports what was created",
    ],
  });

  // Stacking on the selected workspace opens the same dialog, on its branch.
  await page.keyboard.press(`${mod}+Shift+N`);
  await expect(
    page.getByRole("heading", { name: "New Workspaces" }),
  ).toBeVisible();
  await settle(page, 800);
  await capture(page, {
    name: "13-open-workspaces-stack",
    title: "Stack opens the dialog on the selected workspace",
    expectations: [
      "The Stack shortcut opened New Workspaces, not the old Stack dialog",
      "'Stack on' reads treq/retry-flaky-webhook-tests",
    ],
  });
  await page.keyboard.press("Escape");
  await expect(
    page.getByRole("heading", { name: "New Workspaces" }),
  ).toBeHidden();
});

test("open-workspaces: the pull request search shows loading, no matches and errors", async ({
  page,
}) => {
  await home(page);
  await page.getByRole("button", { name: "New workspaces" }).click();
  await expect(
    page.getByRole("heading", { name: "Your pull requests" }),
  ).toBeVisible();
  const search = page.getByLabel("Search pull requests");
  const searching = page.getByRole("progressbar", {
    name: "Searching pull requests",
  });

  // The fake gh holds a search with "slow" in it for 4s.
  await search.fill("slow runbook");
  await expect(searching).toBeVisible();
  await settle(page, 300);
  await capture(page, {
    name: "13-open-workspaces-search-loading",
    title: "Pull request search in flight",
    expectations: [
      "A small grey spinner sits at the right end of the 'Search pull requests' input, which reads 'slow runbook'",
      "The previous list (octocat's PRs) still shows below, dimmed",
    ],
  });

  await expect(searching).toBeHidden({ timeout: 10_000 });
  await expect(
    page.getByRole("checkbox", { name: "#409 docs: add the ingest runbook" }),
  ).toBeVisible();
  await settle(page, 300);
  await capture(page, {
    name: "13-open-workspaces-search-results",
    title: "Pull request search answered",
    expectations: [
      "'Matching pull requests' lists only #409 at full opacity",
      "No spinner in the search input",
    ],
  });

  await search.fill("zzz");
  // The empty state's text isn't exposed; the capture shows it.
  await expect(searching).toBeHidden();
  await expect(
    page.getByRole("checkbox", { name: "#409 docs: add the ingest runbook" }),
  ).toBeHidden();
  await settle(page, 300);
  await capture(page, {
    name: "13-open-workspaces-search-empty",
    title: "Pull request search with no matches",
    expectations: [
      "The list shows a PR icon and 'No pull requests match \"zzz\".', not 'No open pull requests.'",
      "No spinner in the search input",
    ],
  });

  await search.fill("unreachable");
  await expect(page.getByRole("button", { name: "Try again" })).toBeVisible();
  await expect(searching).toBeHidden();
  await settle(page, 300);
  await capture(page, {
    name: "13-open-workspaces-search-error",
    title: "Pull request search failed",
    expectations: [
      "The list shows 'Could not load pull requests' with the gh error 'HTTP 502: Bad Gateway' and a retry action",
      "No spinner in the search input",
    ],
  });

  await page.getByRole("button", { name: "Cancel" }).click();
  await expect(
    page.getByRole("heading", { name: "New Workspaces" }),
  ).toBeHidden();
});

test("open-workspaces: Assigned to me narrows the pull requests to the user's assigned ones", async ({
  page,
}) => {
  await home(page);
  await page.getByRole("button", { name: "New workspaces" }).click();
  await expect(
    page.getByRole("heading", { name: "Your pull requests" }),
  ).toBeVisible();
  const assigned = page.getByRole("button", { name: "Assigned to me" });
  // gpui-playwright reports a toggle's state as `checked`, not `pressed`.
  await expect(
    page.getByRole("button", { name: "Assigned to me", checked: false }),
  ).toBeVisible();
  await settle(page, 500);
  await capture(page, {
    name: "13-open-workspaces-assigned-off",
    title: "Assigned to me, off",
    expectations: [
      "An outline 'Assigned to me' chip with a person icon sits right of the 'Search pull requests' input, the same height",
      "'Your pull requests' lists octocat's own PRs",
    ],
  });

  await assigned.click();
  await expect(
    page.getByRole("heading", { name: "Assigned to you" }),
  ).toBeVisible();
  await expect(
    page.getByRole("button", { name: "Assigned to me", checked: true }),
  ).toBeVisible();
  await expect(
    page.getByRole("checkbox", { name: "#409 docs: add the ingest runbook" }),
  ).toBeVisible();
  await settle(page, 500);
  await capture(page, {
    name: "13-open-workspaces-assigned-on",
    title: "Assigned to me, on",
    expectations: [
      "The 'Assigned to me' chip is drawn in the light selected tint",
      "'Assigned to you' lists #421 by alice and #409 by bob, none of octocat's own",
    ],
  });

  await page.getByLabel("Search pull requests").fill("runbook");
  await expect(
    page.getByRole("heading", { name: "Matching pull requests" }),
  ).toBeVisible();
  await expect(
    page.getByRole("checkbox", { name: /^#421 / }),
  ).toBeHidden();
  await settle(page, 500);
  await capture(page, {
    name: "13-open-workspaces-assigned-search",
    title: "Assigned to me with a search",
    expectations: [
      "The chip stays on; 'Matching pull requests' lists only #409",
    ],
  });

  await page.getByRole("button", { name: "Cancel" }).click();
  await expect(
    page.getByRole("heading", { name: "New Workspaces" }),
  ).toBeHidden();
});

test("open-workspaces: indented lines stack on the line above", async ({
  page,
}) => {
  await home(page);
  await page.getByRole("button", { name: "New workspaces" }).click();
  await expect(
    page.getByRole("heading", { name: "New Workspaces" }),
  ).toBeVisible();
  const lines = page.getByLabel("Workspaces", { exact: true });
  await lines.fill(
    ["Add the audit log", "Audit log API", "Audit log page"].join("\n"),
  );
  // Select the last two lines and indent them with Tab.
  await page.keyboard.press("Shift+ArrowUp");
  await page.keyboard.press("Tab");
  await expect(page.getByRole("status")).toHaveAccessibleName(
    "3 workspaces will be created",
    {
      timeout: 20_000,
    },
  );
  await settle(page, 500);
  await capture(page, {
    name: "13-open-workspaces-indent-typed",
    title: "Tab indents the selected lines",
    expectations: [
      "The textarea shows 'Audit log API' and 'Audit log page' indented two spaces under 'Add the audit log'",
      "Status reads '3 workspaces will be created'; button reads 'Create 3 Workspaces'",
    ],
  });
  await page.getByRole("status").click();
  await expect(
    page.getByRole("list", { name: "Workspaces to open" }),
  ).toBeVisible();
  await settle(page, 500);
  await capture(page, {
    name: "13-open-workspaces-indent",
    title: "Indented lines stack on the line above",
    expectations: [
      "The popover lists treq/add-the-audit-log new branch; treq/audit-log-api on treq/add-the-audit-log; treq/audit-log-page on treq/audit-log-api",
    ],
  });
  await page.keyboard.press("Escape");
  await page.getByRole("button", { name: "Create 3 Workspaces" }).click();
  await expect(
    page.getByRole("heading", { name: "New Workspaces" }),
  ).toBeHidden({
    timeout: 30_000,
  });
  for (const branch of [
    "treq/add-the-audit-log",
    "treq/audit-log-api",
    "treq/audit-log-page",
  ]) {
    await expect(row(page, branch)).toBeVisible({ timeout: 20_000 });
  }
  await settle(page, 1500);
  await capture(page, {
    name: "13-open-workspaces-indent-opened",
    title: "The stack in the sidebar",
    clip: { x: 0, y: 0, width: 300, height: 520 },
    expectations: [
      "The sidebar shows treq/audit-log-api stacked under treq/add-the-audit-log, and treq/audit-log-page under treq/audit-log-api",
    ],
  });
});
