// The desktop Linear integration against the fake Linear (./fake-linear.ts):
// connecting it from Settings with an API key, the Linear view (issue
// views, an expanded issue with its comments, Kanban, the team menu,
// projects with a document), Kick off into the agent prompt, a workspace
// created from an issue in the New Workspaces dialog with its header
// chip, and a key Linear rejects.
//
// Issue rows and project rows are not observable; they are clicked at
// their place in the 1400x900 window.
import { createHash } from "node:crypto";
import { join } from "node:path";
import { DatabaseSync } from "node:sqlite";
import { expect, test, type Page } from "@playwright/test";
import { fixture } from "../fixture.ts";
import { capture } from "./capture.ts";
import { home, ready, settle } from "./helpers.ts";

test.describe.configure({ mode: "serial" });

/** Sets the QA repository's Linear API key (`Database::make_repo_key`);
 * `null` removes it. */
function linearKey(value: string | null): void {
  const { home, repo } = fixture();
  const hash = createHash("sha256").update(repo).digest("hex").slice(0, 16);
  const key = `repo_${hash}_linear_api_key`;
  const db = new DatabaseSync(join(home, ".local/share/com.treq/treq.db"));
  if (value === null) db.prepare("DELETE FROM settings WHERE key = ?").run(key);
  else db.prepare("INSERT OR REPLACE INTO settings (key, value) VALUES (?, ?)").run(key, value);
  db.close();
}

test.afterAll(() => linearKey(null));

async function linear(page: Page): Promise<void> {
  await ready(page);
  const close = page.getByRole("button", { name: "Close", exact: true });
  if ((await close.count()) > 0) await close.first().click();
  await page.getByRole("button", { name: "Linear", exact: true }).click();
  await expect(page.getByRole("button", { name: "Linear", selected: true })).toBeVisible();
  await page.mouse.move(1000, 860);
  await settle(page, 2000);
}

test("linear: connect with an API key in Settings", async ({ page }) => {
  linearKey(null);
  await linear(page);
  await expect(page.getByText(/Linear is not connected/)).toBeVisible();
  await page.getByRole("button", { name: "Open Settings" }).click();
  await expect(page.getByRole("tab", { name: "Integrations", selected: true })).toBeVisible();
  await page.getByRole("button", { name: "Add API Key" }).click();
  await settle(page, 600);
  await page.getByRole("textbox").last().click();
  await page.keyboard.type("lin_api_qa");
  await settle(page, 400);
  await capture(page, {
    name: "22-linear-01-key-entry",
    title: "Settings: typing a Linear API key",
    expectations: [
      "Linear card on the Integrations tab: a masked key field (typed, focused), Cancel and an enabled Save",
      "The field and Save sit on the API Key row, aligned with its label",
    ],
  });
  await page.getByRole("button", { name: "Save" }).click();
  await expect(page.getByRole("button", { name: "Update API Key" })).toBeVisible();
  await settle(page, 800);
  await capture(page, {
    name: "22-linear-02-key-saved",
    title: "Settings: Linear connected",
    screen: true,
    expectations: [
      "API Key row reads 'Direct API access configured' with an 'Update API Key' button",
      "An 'API Key saved' toast",
    ],
  });
});

test("linear: issues", async ({ page }) => {
  linearKey("lin_api_qa");
  await linear(page);
  await expect(page.getByRole("tab", { name: "All Issues", selected: true })).toBeVisible();
  await settle(page, 1500);
  await capture(page, {
    name: "22-linear-03-all-issues",
    title: "Linear view: All Issues",
    expectations: [
      "Header: Linear, an 'All Teams' selector, Issues / Projects tabs; view tabs, List / Kanban, Filter and Refresh",
      "Six rows (ENG-143, ENG-142, ENG-139, ENG-131, ENG-128, ENG-125) with state, priority, labels, project and assignee",
      "Each row has a Kick off button; no spinner or error",
    ],
  });

  await page.getByRole("tab", { name: "My Issues" }).click();
  await settle(page, 1000);
  await capture(page, {
    name: "22-linear-04-my-issues",
    title: "Linear view: My Issues",
    expectations: ["Only octocat's issues: ENG-143, ENG-142, ENG-139, ENG-131, ENG-128 (not hubot's ENG-125)"],
  });
  await page.getByRole("tab", { name: "All Issues" }).click();
  await settle(page, 800);

  // Expand ENG-142 (the second row) for its description and comments.
  await page.mouse.click(700, 186);
  await settle(page, 2000);
  await capture(page, {
    name: "22-linear-05-issue-expanded",
    title: "Linear view: an expanded issue",
    expectations: [
      "The clicked row expands to show its description and a COMMENTS column",
      "Two comments (hubot, octocat) with authors and times; a 'Review with agent' button",
    ],
  });
  await page.mouse.click(700, 186);
  await settle(page, 800);

  await page.getByRole("tab", { name: "Kanban" }).click();
  await settle(page, 1200);
  await capture(page, {
    name: "22-linear-06-kanban",
    title: "Linear view: Kanban",
    expectations: [
      "Columns Backlog, Todo, In Progress, Done in workflow order, cards under each",
      "ENG-143's card says it is a sub-issue of ENG-142; label chips on cards",
    ],
  });
  await page.getByRole("tab", { name: "List" }).click();
  await settle(page, 800);

  await page.getByRole("button", { name: "All Teams" }).click();
  await settle(page, 1200);
  await capture(page, {
    name: "22-linear-07-team-menu",
    title: "Linear view: team menu",
    screen: true,
    expectations: ["A menu under the selector: Teams heading, All Teams, Engineering, Operations"],
  });
  await page.keyboard.press("Escape");
  await settle(page, 600);
});

test("linear: projects", async ({ page }) => {
  linearKey("lin_api_qa");
  await linear(page);
  await page.getByRole("tab", { name: "Projects", exact: true }).click();
  await settle(page, 2000);
  await capture(page, {
    name: "22-linear-08-projects",
    title: "Linear view: Projects",
    expectations: [
      "Ingest, Docs and API listed with status and lead; view tabs All / Mine / Active / Backlog",
      "The first project, Ingest, is selected with its detail on the right",
    ],
  });
  // The first project is selected; open its document.
  await page.mouse.click(631, 228);
  await settle(page, 1500);
  await capture(page, {
    name: "22-linear-09-project-document",
    title: "Linear view: a project's document",
    screen: true,
    expectations: [
      "A dialog titled 'Retry policy' renders the document's markdown (heading, bold 5)",
      "Its comments column reads 'No comments'",
    ],
  });
  await page.keyboard.press("Escape");
  await settle(page, 800);
  await page.getByRole("tab", { name: "Backlog", exact: true }).click();
  await settle(page, 1200);
  await capture(page, {
    name: "22-linear-09b-projects-backlog",
    title: "Linear view: Backlog projects",
    expectations: ["The Backlog view lists projects not started yet: Docs (planned) and API (backlog)"],
  });
  await page.getByRole("tab", { name: "All", exact: true }).click();
  await page.getByRole("tab", { name: "Issues", exact: true }).click();
  await settle(page, 800);
});

test("linear: kick off an issue", async ({ page }) => {
  linearKey("lin_api_qa");
  await linear(page);
  await page.getByRole("tab", { name: "Issues", exact: true }).click();
  await page.getByRole("tab", { name: "List" }).click();
  await settle(page, 1000);
  await page.getByRole("button", { name: "Kick off" }).first().click();
  await settle(page, 1500);
  await capture(page, {
    name: "22-linear-10-kickoff",
    title: "Kick off: the agent prompt with the issue",
    screen: true,
    expectations: [
      "The agent prompt dialog with a 'Linear · ENG-142' chip",
      "A note says it opens a workspace for ENG-142",
    ],
  });
  await page.keyboard.press("Escape");
  await settle(page, 600);
  await page.keyboard.press("Escape");
  await settle(page, 600);
});

test("linear: new workspace from an issue", async ({ page }) => {
  linearKey("lin_api_qa");
  await home(page);
  await page.getByRole("button", { name: "New workspaces" }).click();
  await expect(page.getByRole("heading", { name: "New Workspaces" })).toBeVisible();
  await page.getByRole("tab", { name: "Linear" }).click();
  await expect(page.getByRole("heading", { name: "Your issues" })).toBeVisible({ timeout: 20_000 });
  await page.getByRole("textbox", { name: "Search issues" }).click();
  await page.keyboard.type("cache");
  await settle(page, 800);
  await page.getByRole("checkbox", { name: "ENG-139 Cache key-value lookups" }).click();
  await settle(page, 600);
  await capture(page, {
    name: "22-linear-11-dialog-search",
    title: "New Workspaces: searching Linear issues",
    expectations: [
      "Heading 'Matching issues' with only ENG-139 listed and checked",
      "Linear tab shows a '1' badge; button reads 'Create 1 Workspace'",
    ],
  });
  await page.getByRole("button", { name: /^Create 1 Workspace/ }).click();
  await expect(page.getByRole("heading", { name: "New Workspaces" })).toBeHidden({ timeout: 30_000 });
  // The new workspace opens, its sidebar row named by the issue title.
  await expect(
    page.getByRole("button", { name: "Cache key-value lookups", selected: true }),
  ).toBeVisible({ timeout: 20_000 });
  await settle(page, 2000);
  await capture(page, {
    name: "22-linear-12-workspace-chip",
    title: "Workspace created from ENG-139",
    expectations: [
      "The sidebar selects the new 'Cache key-value lookups' row; the header branch is octocat/eng-139-…",
      "The workspace header shows an ENG-139 Linear chip",
    ],
  });
});

test("linear: a rejected key", async ({ page }) => {
  linearKey("lin_api_bad");
  await linear(page);
  await settle(page, 1500);
  await capture(page, {
    name: "22-linear-13-rejected-key",
    title: "Linear view: Linear rejects the key",
    expectations: [
      "The issues pane says Linear rejected the API key, with an Open Settings button",
      "No stale issue rows from the earlier, valid key",
    ],
  });
  await page.getByRole("tab", { name: "Projects", exact: true }).click();
  await settle(page, 1500);
  await capture(page, {
    name: "22-linear-14-rejected-key-projects",
    title: "Linear view: Projects with a rejected key",
    expectations: ["The projects pane shows the same rejection with a way to fix it (Open Settings)"],
  });
  await home(page);
  await page.getByRole("button", { name: "New workspaces" }).click();
  await page.getByRole("tab", { name: "Linear" }).click();
  await settle(page, 2000);
  await capture(page, {
    name: "22-linear-15-rejected-key-dialog",
    title: "New Workspaces: Linear rejects the key",
    expectations: ["'Could not load Linear issues' with the rejection message and a Try again button"],
  });
  await page.keyboard.press("Escape");
  await settle(page, 600);
});
