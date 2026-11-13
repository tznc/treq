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
  ]);
});

test("issue detail", async () => {
  await page.getByText("Empty event payloads drop the Discord body").click();
  await expect(page.getByRole("button", { name: "Comment", exact: true })).toBeVisible();
  await shot("02-issue", "Issue #77", ["The title, an Open tag, #77 and its author alice"]);
  await page.getByRole("button", { name: "Back", exact: true }).click();
});

test("pull requests list", async () => {
  await page.getByRole("tab", { name: "Pull requests" }).click();
  await expect(page.getByText("feat: handle empty event messages")).toBeVisible();
  await shot("03-prs", "Pull requests tab", [
    "Open pull requests with their head → base branches and state tags",
  ]);
});

test("pull request detail and review", async () => {
  await page.getByText("feat: handle empty event messages").click();
  await expect(page.getByRole("button", { name: "Review changes" })).toBeVisible();
  await shot("04-pr", "Pull request #418", [
    "The title, state and review tags, feat/empty-event-message → feat/event-ingest, and the checks summary with one failing",
  ]);
  await page.getByRole("button", { name: "Review changes" }).click();
  await expect(page.getByRole("button", { name: "Submit review" })).toBeVisible();
  await page.waitForTimeout(1500);
  await capture(page, {
    name: "mobile-github-05-review-light",
    title: "Review #418",
    expectations: ["The review page with Submit review as its one filled button"],
    issue: "The fake gh serves no `gh pr diff`, so the file list is empty",
  });
});
