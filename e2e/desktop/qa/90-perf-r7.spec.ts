// Round 7 desktop UI performance, before/after: long comment threads fold
// behind "Show N earlier comments" (GitHub PR, Linear issue), the Logs chart
// with a deep history, and a long PR list.
import { createHash } from "node:crypto";
import { rmSync, writeFileSync } from "node:fs";
import { join } from "node:path";
import { DatabaseSync } from "node:sqlite";
import { expect, test, type Page } from "@playwright/test";
import { fixture } from "../fixture.ts";
import { capture } from "./capture.ts";
import { openTab, openWorkspace, ready, settle } from "./helpers.ts";

test.describe.configure({ mode: "serial" });

const PRS_TAB = { x: 422, y: 62 };
const FIRST_ROW = { x: 700, y: 147 };
const prsFile = () => join(fixture().home, "fake-gh-prs.json");
const extraFile = () => join(fixture().home, "fake-gh-extra-prs.json");

test.afterAll(() => {
  rmSync(prsFile(), { force: true });
  rmSync(extraFile(), { force: true });
  const { home, repo } = fixture();
  const hash = createHash("sha256").update(repo).digest("hex").slice(0, 16);
  const db = new DatabaseSync(join(home, ".local/share/com.treq/treq.db"));
  db.prepare("DELETE FROM settings WHERE key = ?").run(`repo_${hash}_linear_api_key`);
  db.close();
});

async function scrollMain(page: Page, dy: number): Promise<void> {
  await page.mouse.move(900, 450);
  await page.mouse.wheel(0, dy);
  await settle(page, 700);
}

async function githubPrs(page: Page): Promise<void> {
  await ready(page);
  await page.getByRole("button", { name: "Github", exact: true }).click();
  await settle(page, 2000);
  await page.mouse.click(PRS_TAB.x, PRS_TAB.y);
  await settle(page, 2000);
}

test("perf-r7: PR with a long comment thread", async ({ page }) => {
  const comments = Array.from({ length: 24 }, (_, i) => ({
    id: `IC_long_${i + 1}`,
    body:
      i % 4 === 3
        ? `Comment ${i + 1}: a longer note.\n\n- first point\n- second point with \`inline code\`\n\nClosing line.`
        : `Comment ${i + 1}: ${i % 2 ? "Rebased onto main, tests are green." : "Looks good to me."}`,
    author: { login: i % 3 ? "hubot" : "alice", avatarUrl: null },
    createdAt: new Date(Date.UTC(2026, 7, 1 + (i % 12), 8 + (i % 9), i)).toISOString(),
    reactionGroups: [],
  }));
  writeFileSync(prsFile(), JSON.stringify({ "feat/empty-event-message": { comments } }));
  await githubPrs(page);
  await page.mouse.click(FIRST_ROW.x, FIRST_ROW.y);
  await expect(page.getByRole("button", { name: "Review changes" })).toBeVisible();
  await settle(page, 2500);
  await scrollMain(page, 900);
  await capture(page, {
    name: "github-pr-comments-folded",
    expectations: [
      "After: first comments, a 'Show N earlier comments' row, then the latest; before: all 24 comments",
      "Fold row aligned with the comment column",
    ],
  });
  const more = page.getByRole("button", { name: /earlier comments/ });
  if ((await more.count()) > 0) {
    await more.first().click();
    await settle(page, 1200);
  }
  await capture(page, {
    name: "github-pr-comments-expanded",
    expectations: ["All 24 comments in order; the fold row is gone"],
  });
});

test("perf-r7: Linear issue with a long comment thread", async ({ page }) => {
  const { home, repo } = fixture();
  const hash = createHash("sha256").update(repo).digest("hex").slice(0, 16);
  const db = new DatabaseSync(join(home, ".local/share/com.treq/treq.db"));
  db.prepare("INSERT OR REPLACE INTO settings (key, value) VALUES (?, ?)").run(
    `repo_${hash}_linear_api_key`,
    "lin_api_qa",
  );
  db.close();
  await ready(page);
  const close = page.getByRole("button", { name: "Close", exact: true });
  if ((await close.count()) > 0) await close.first().click();
  await page.getByRole("button", { name: "Linear", exact: true }).click();
  await expect(page.getByRole("button", { name: "Linear", selected: true })).toBeVisible();
  await page.mouse.move(1000, 860);
  await settle(page, 2500);
  await page.getByRole("listitem", { name: "Cache key-value lookups" }).click();
  await expect(page.getByRole("button", { name: "Back to issues" })).toBeVisible();
  await settle(page, 2500);
  await scrollMain(page, 500);
  await capture(page, {
    name: "linear-comments-folded",
    expectations: [
      "After: first comments, 'Show N earlier comments' row, then the latest; before: all 30",
      "Fold row aligned with the comment bubbles",
    ],
  });
  const more = page.getByRole("button", { name: /earlier comments/ });
  if ((await more.count()) > 0) {
    await more.first().click();
    await settle(page, 1200);
  }
  await capture(page, {
    name: "linear-comments-expanded",
    expectations: ["All 30 comments in order; the fold row is gone"],
  });
});

test("perf-r7: Logs chart with a deep history", async ({ page }) => {
  const { repo } = fixture();
  const day = new Date().toISOString().slice(0, 10);
  const db = new DatabaseSync(join(repo, ".treq", `telemetry-${day}.db`));
  db.exec(`CREATE TABLE IF NOT EXISTS logs (
    timestamp TEXT NOT NULL, observed_timestamp TEXT NOT NULL, trace_id TEXT NOT NULL,
    span_id TEXT NOT NULL, trace_flags INTEGER NOT NULL, severity_text TEXT NOT NULL,
    severity_number INTEGER NOT NULL, body TEXT NOT NULL, resource TEXT NOT NULL,
    instrumentation_scope TEXT NOT NULL, attributes TEXT NOT NULL, event_name TEXT NOT NULL,
    run_id INTEGER NOT NULL, job_id TEXT NOT NULL, step_index INTEGER NOT NULL,
    step_name TEXT NOT NULL, log_iostream TEXT NOT NULL)`);
  const insert = db.prepare(
    "INSERT INTO logs VALUES (?,?,?,?,0,?,?,?,'{}','{}','{}','log',?,?,?,?, 'stdout')",
  );
  const now = Date.now();
  db.exec("BEGIN");
  for (let i = 0; i < 30000; i++) {
    // About 6 hours: a steady trickle plus a dense burst in the middle.
    const frac = i % 3 ? ((i * 7919) % 30000) / 30000 : 0.45 + (((i * 104729) % 1000) / 1000) * 0.1;
    const t = new Date(now - 6 * 3600_000 + Math.floor(frac * 21_600_000));
    const sev = i % 400 === 0 ? ["ERROR", 17] : i % 40 === 0 ? ["WARN", 13] : ["INFO", 9];
    const iso = t.toISOString();
    insert.run(iso, iso, "", "", sev[0], sev[1], JSON.stringify({ message: `step output line ${i}` }), 1 + (i % 7), `job-${i % 3}`, i % 5, `step ${i % 5}`);
  }
  db.exec("COMMIT");
  db.close();
  await openWorkspace(page, "feat/keyvalues-cache");
  await openTab(page, "Logs");
  await settle(page, 4000);
  await capture(page, {
    name: "logs-chart",
    expectations: [
      "Chart of Info/Warn/Error stacks above the feed, bars evenly spaced",
      "Feed rows below the chart, nothing overlapping",
    ],
  });
});

test("perf-r7: long PR list", async ({ page }) => {
  const extra = Array.from({ length: 70 }, (_, i) => ({
    number: 500 + i,
    title: `chore: batch change ${i + 1} to the ingest pipeline${i % 5 === 0 ? " with a rather long title that should truncate" : ""}`,
    headRefName: `chore/batch-${i + 1}`,
    baseRefName: "main",
    mergeStateStatus: "CLEAN",
    state: "OPEN",
    url: `https://github.com/lorem-ipsum/event-bus/pull/${500 + i}`,
    author: { login: i % 2 ? "alice" : "bob", avatarUrl: null },
    labels: [],
    createdAt: "2026-08-01T10:00:00Z",
    updatedAt: "2026-08-12T10:00:00Z",
    isDraft: i % 9 === 0,
    body: "",
    comments: [],
  }));
  writeFileSync(extraFile(), JSON.stringify(extra));
  await githubPrs(page);
  await settle(page, 2000);
  await capture(page, {
    name: "github-pr-list",
    expectations: [
      "PR list with ~75 rows, rows evenly spaced and aligned",
      "Title, number, badges, author, branches, date columns consistent",
    ],
  });
  await scrollMain(page, 1500);
  await capture(page, {
    name: "github-pr-list-scrolled",
    expectations: ["Rows mid-list render fully, no gaps or blank bands"],
  });
});
