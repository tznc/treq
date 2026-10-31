// The desktop Linear integration against the fake Linear (./fake-linear.ts):
// connecting it from Settings with an API key, the Linear view (issue
// views, an issue's page with its comments, Kanban, the team menu,
// projects with a document), the agent button into the agent prompt, a
// workspace created from an issue in the New workspace dialog with its
// header chip, and a key Linear rejects.
//
// Project rows and documents are not observable; they are clicked at
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
  await page.getByRole("button", { name: "Open settings" }).click();
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
  await expect(page.getByRole("tab", { name: "All issues", selected: true })).toBeVisible();
  await settle(page, 1500);
  await capture(page, {
    name: "22-linear-03-all-issues",
    title: "Linear view: All Issues",
    expectations: [
      "Header: Linear, an 'All teams' selector, Issues / Projects tabs; view tabs, List / Kanban, Filter and Refresh",
      "Two-line rows: the title, then identifier · state · priority · project · labels · assignee",
      "ENG-143 indented under ENG-142; a robot agent button at the right of each row; no spinner or error",
    ],
  });

  await page.getByRole("tab", { name: "My issues" }).click();
  await settle(page, 1000);
  await capture(page, {
    name: "22-linear-04-my-issues",
    title: "Linear view: My Issues",
    expectations: [
      "Only octocat's issues: ENG-143, ENG-142, ENG-139, ENG-131, ENG-128 (not hubot's ENG-125)",
      "Same two-line rows as All Issues",
    ],
  });
  await page.getByRole("tab", { name: "All issues" }).click();
  await settle(page, 800);

  await page.getByRole("listitem", { name: "Retry webhook deliveries" }).click();
  await expect(page.getByRole("button", { name: "Back to issues" })).toBeVisible();
  await settle(page, 2000);
  await capture(page, {
    name: "22-linear-05-issue-page",
    title: "Linear view: an issue's page",
    expectations: [
      "The list gives way to ENG-142's page: Back, the identifier, the agent button and Open in Linear",
      "Title, state/priority/project/assignee badges, the Bug label, the description",
      "COMMENTS under the description: hubot's and octocat's bubbles with times",
    ],
  });
  await page.getByRole("button", { name: "Back to issues" }).click();
  await settle(page, 800);

  await page.getByRole("tab", { name: "Kanban" }).click();
  await settle(page, 1200);
  await capture(page, {
    name: "22-linear-06-kanban",
    title: "Linear view: Kanban",
    expectations: [
      "Columns Backlog, Todo, In Progress, Done in workflow order, cards under each",
      "Each card: identifier with a robot agent button on the right, title, label chips",
      "ENG-143's card says it is a sub-issue of ENG-142",
    ],
  });
  await page.getByRole("listitem", { name: "Cache key-value lookups" }).click();
  await expect(page.getByRole("button", { name: "Back to issues" })).toBeVisible();
  await settle(page, 1500);
  await capture(page, {
    name: "22-linear-06b-kanban-card-page",
    title: "Linear view: a Kanban card opens the issue's page",
    expectations: ["ENG-139's page, with 'No comments' under its description"],
  });
  await page.getByRole("button", { name: "Back to issues" }).click();
  await settle(page, 800);
  await page.getByRole("tab", { name: "List" }).click();
  await settle(page, 800);

  await page.getByRole("button", { name: "All teams" }).click();
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
      "Ingest has no comments, so no comments section shows under its description",
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
      "hubot's comment shows as a bubble right under the paragraph it quotes, 'exponential backoff' highlighted",
      "No comments column",
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
  await page.getByRole("button", { name: "Start agent" }).first().click();
  await settle(page, 1500);
  await capture(page, {
    name: "22-linear-10-kickoff",
    title: "Agent button: the agent prompt with the issue",
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
  await page.getByRole("button", { name: "New workspace…" }).first().click();
  await expect(page.getByRole("heading", { name: "New workspace" })).toBeVisible();
  await page.getByRole("tab", { name: "Linear" }).click();
  await expect(page.getByRole("heading", { name: "Your issues" })).toBeVisible({ timeout: 20_000 });
  await page.getByRole("textbox", { name: "Search issues" }).click();
  await page.keyboard.type("cache");
  await settle(page, 800);
  await page.getByRole("checkbox", { name: "ENG-139 Cache key-value lookups" }).click();
  await settle(page, 600);
  await capture(page, {
    name: "22-linear-11-dialog-search",
    title: "New workspace: searching Linear issues",
    expectations: [
      "Heading 'Matching issues' with only ENG-139 listed and checked",
      "Linear tab shows a '1' badge; button reads 'Create workspace'",
    ],
  });
  await page.getByRole("button", { name: /^Create workspace/ }).click();
  await expect(page.getByRole("heading", { name: "New workspace" })).toBeHidden({ timeout: 30_000 });
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
      "An ENG-139 chip, without an icon, sits on the right of the header next to the checks/PR buttons",
    ],
  });
});

test("linear: a sub-issue nests under its parent's workspace", async ({ page }) => {
  linearKey("lin_api_qa");
  await home(page);
  await page.getByRole("button", { name: "New workspaces" }).click();
  await expect(page.getByRole("heading", { name: "New workspace" })).toBeVisible();
  await page.getByRole("tab", { name: "Linear" }).click();
  await expect(page.getByRole("heading", { name: "Your issues" })).toBeVisible({ timeout: 20_000 });
  // The sub-issue first: its parent still opens before it.
  await page.getByRole("checkbox", { name: "ENG-143 Back off on 429 responses" }).click();
  await page.getByRole("checkbox", { name: "ENG-142 Retry webhook deliveries" }).click();
  await settle(page, 600);
  await page.getByRole("button", { name: /^Create 2 workspaces/ }).click();
  await expect(page.getByRole("heading", { name: "New workspace" })).toBeHidden({ timeout: 60_000 });
  await expect(
    page.getByRole("button", { name: "Retry webhook deliveries" }).first(),
  ).toBeVisible({ timeout: 30_000 });
  await expect(
    page.getByRole("button", { name: "Back off on 429 responses" }).first(),
  ).toBeVisible({ timeout: 30_000 });
  await settle(page, 2500);
  await capture(page, {
    name: "22-linear-16-sub-issue-nested",
    title: "Workspaces created from ENG-142 and its sub-issue ENG-143",
    expectations: [
      "The sidebar lists 'Retry webhook deliveries' (ENG-142) with 'Back off on 429 responses' (ENG-143) nested and indented under it",
      "ENG-143 is not a sibling of ENG-142 beside the default branch's other workspaces",
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
      "One empty state fills the view: the Linear logo, 'Linear rejected the API key', the reason and Open Settings",
      "No Issues/Projects tabs, team selector or stale issue rows",
    ],
  });
  await home(page);
  await page.getByRole("button", { name: "New workspace…" }).first().click();
  await page.getByRole("tab", { name: "Linear" }).click();
  await settle(page, 2000);
  await capture(page, {
    name: "22-linear-15-rejected-key-dialog",
    title: "New workspace: Linear rejects the key",
    expectations: ["'Could not load Linear issues' with the rejection message and a Try again button"],
  });
  await page.keyboard.press("Escape");
  await settle(page, 600);
});
