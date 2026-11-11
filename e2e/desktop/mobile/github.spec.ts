// The mobile GitHub screens against a real SSH host: the host's `treq`
// answers the app's reads and its fake `gh` (qa/ssh-server/up.sh with
// QA_SSH_FAKE_GH=1) stands in for GitHub. Opt-in, like 18-ssh-remote:
// set the QA_SSH_* variables up.sh prints.
import { expect, test, type Page } from "@playwright/test";
import { capture } from "../qa/capture.ts";
import { addHost, host, launchMobile, openGithub, seedRepo, type MobileApp } from "./app.ts";

let m: MobileApp;
let page: Page;

test.describe.configure({ mode: "serial" });
test.skip(!host.host, "QA_SSH_HOST is not set");

test.beforeAll(async () => {
  seedRepo();
  m = await launchMobile();
  page = m.page;
  await addHost(page);
  await openGithub(page);
});

test.afterAll(async () => {
  await m?.browser.close().catch(() => {});
  m?.app.kill("SIGTERM");
});

/** Taps a sheet's entry and waits for that sheet to close: the app guards
 * taps for a moment as a sheet closes, as a person's next tap would wait. */
async function pick(name: string): Promise<void> {
  await page.getByRole("menuitem", { name, exact: true }).click();
  await page.waitForTimeout(800);
}

async function shot(name: string, title: string, expectations: string[]): Promise<void> {
  await page.waitForTimeout(1500);
  if (process.env.QA_DEBUG) console.log(`--- ${name}\n${await page.ariaSnapshot()}`);
  await capture(page, { name: `mobile-github-${name}-light`, title, expectations });
}

test("issues list", async () => {
  await expect(page.getByText("Empty event payloads drop the Discord body")).toBeVisible();
  await shot("01-issues", "Issues tab", [
    "The Issues tab is selected and lists the host's open issues, newest first",
    "One filled button only: New issue at the bottom",
    "The filter row: an outline 'Open' with a caret, then Filters",
  ]);
});

test("issue filters", async () => {
  await page.getByRole("button", { name: "State: Open" }).click();
  await expect(page.getByRole("menuitem", { name: "All" })).toBeVisible();
  await shot("01a-issues-state", "Issues: State menu", [
    "A State sheet lists Open, Closed and All; Open has the light selected tint",
  ]);
  await pick("Open");
  await page.getByRole("button", { name: "Filters" }).click();
  await pick("Author: Any");
  await pick("alice");
  await expect(page.getByRole("button", { name: "Filters · 1" })).toBeVisible();
  await expect(page.getByText("Document retry/backoff for ingest")).toHaveCount(0);
  await shot("01b-issues-author", "Issues: Author = alice", [
    "The Filters button reads 'Filters · 1'",
    "Only alice's issues (#77 and #58) are listed",
  ]);
  await page.getByRole("button", { name: "Filters · 1" }).click();
  await pick("Clear filters");
});

test("issue detail and composer", async () => {
  await page.getByText("Empty event payloads drop the Discord body").click();
  const bar = page.getByRole("button", { name: "Add a comment…" });
  await expect(bar).toBeVisible();
  // Closing moved to the ⋯ menu: the foot has the bar and nothing else.
  await expect(page.getByRole("button", { name: "Close…" })).toHaveCount(0);
  await shot("02-issue", "Issue #77", [
    "The title, an Open tag, #77 and its author alice",
    "The foot is one rounded 'Add a comment…' bar; no Close… or Comment buttons",
  ]);
  await bar.click();
  const send = page.getByRole("button", { name: "Comment", exact: true });
  await expect(send).toBeVisible();
  await shot("02a-composer", "Composer sheet", [
    "A sheet titled 'Comment on #77' with Write (selected) and Preview, a text box and one filled Comment button",
  ]);
  await page.keyboard.type("Shipped a fix in **#418**: empty bodies now fall back to the JSON payload.");
  await page.getByRole("button", { name: "Preview" }).click();
  await expect(page.getByText("empty bodies now fall back")).toBeVisible();
  await shot("02b-preview", "Composer preview", [
    "Preview is selected and shows the comment rendered, with #418 in bold",
  ]);
  // A tap on the dimmed page above the sheet closes it; the text stays.
  await page.mouse.click(210, 60);
  await page.waitForTimeout(800);
  await expect(page.getByRole("button", { name: /^Continue comment: Shipped a fix/ })).toBeVisible();
  await shot("02c-unsent", "Unsent comment kept", [
    "The bar shows the start of the unsent comment in body colour",
  ]);
  await page.getByRole("button", { name: "Back", exact: true }).click();
});

test("pull requests list", async () => {
  await page.getByRole("tab", { name: "Pull requests" }).click();
  await expect(page.getByText("feat: handle empty event messages")).toBeVisible();
  await shot("03-prs", "Pull requests tab", [
    "The filter row (State: Open, Filters) sits above the search box",
    "Open pull requests with their head → base branches and state tags",
  ]);
  await page.getByRole("button", { name: "Filters" }).click();
  await expect(page.getByRole("menuitem", { name: "Only mine: Off" })).toBeVisible();
  await shot("03a-prs-filters", "Pull requests: Filters sheet", [
    "Only mine, Author, Label, Base and Conflicts, each showing its value",
  ]);
  // A tap on the dimmed page above a sheet closes it.
  await page.mouse.click(210, 60);
  await page.waitForTimeout(800);
});

test("pull request detail and review", async () => {
  await page.getByText("feat: handle empty event messages").click();
  await expect(page.getByRole("button", { name: "Review changes" })).toBeVisible();
  await expect(page.getByRole("button", { name: "Add a comment…" })).toBeVisible();
  await shot("04-pr", "Pull request #418", [
    "The 'Add a comment…' bar sits above Review changes; no comment box in the page",
    "The title, state and review tags, feat/empty-event-message → feat/event-ingest, and the checks summary with one failing",
  ]);
  await page.getByRole("button", { name: "Review changes" }).click();
  await expect(page.getByRole("button", { name: "Submit review" })).toBeVisible();
  await expect(page.getByText("web/src/events/post.ts")).toBeVisible();
  await shot("05-review", "Review #418", [
    "1 FILE: web/src/events/post.ts with +4 −1",
    "Submit review is the one filled button",
  ]);
  await page.getByText("web/src/events/post.ts").click();
  await expect(page.getByRole("button", { name: "Files" })).toBeVisible();
  await shot("06-review-file", "Review #418: the file's diff", [
    "The hunk @@ -1,5 +1,7 @@ with one removed (red) and four added (green) lines, numbered",
    "Previous, Files and Next above Submit review; Previous and Next are disabled (one file)",
  ]);
});
