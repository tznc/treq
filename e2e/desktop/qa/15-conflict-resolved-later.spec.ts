// A conflicted commit stops being flagged once a later commit in the stack
// resolves it: jj keeps the original commit's tree conflicted, so the
// Commits tab reads whether the conflict still reaches the stack's tip.
// Commits the resolution to the seed's conflicted workspace, so it runs
// after the specs that show that conflict.
import { execFileSync } from "node:child_process";
import { join } from "node:path";
import { expect, test } from "@playwright/test";
import { fixture } from "../fixture.ts";
import { appBinary } from "../env.ts";
import { appEnv } from "../harness.ts";
import { MARKETING, writeInWorkspace } from "../landing/seed.ts";
import { capture } from "./capture.ts";
import { openTab, openWorkspace, settle } from "./helpers.ts";

const RESOLVED_HOME = `import { EventFeed } from "../components/EventFeed";

export function HomePage() {
  return (
    <main>
      <h1>Event Bus</h1>
      <p>Events from Discord and Slack, for ingest owners.</p>
      <EventFeed source="discord" />
    </main>
  );
}
`;

test("conflict resolved later: the original commit loses its badge", async ({ page }) => {
  await openWorkspace(page, MARKETING);
  await openTab(page, "Commits");
  await settle(page, 1500);
  await expect(page.getByRole("button", { name: "Resolve conflicts…" })).toBeVisible();

  const f = fixture();
  writeInWorkspace(f.repo, MARKETING, "packages/web/src/pages/Home.tsx", RESOLVED_HOME);
  execFileSync(appBinary, ["commit", MARKETING, "-m", "fix: resolve home page conflict"], {
    cwd: f.repo,
    env: {
      ...appEnv({ ...f, bin: join(f.root, "bin") }),
      TREQ_APP_DATA_DIR: join(f.home, ".local/share/com.treq"),
    },
    stdio: "pipe",
  });

  await openTab(page, "Code");
  await openTab(page, "Commits");
  await settle(page, 2500);
  await capture(page, {
    name: "15-conflict-resolved-later",
    title: "Commits tab after a later commit resolves the conflict",
    expectations: [
      "'fix: resolve home page conflict' sits on top of the stack",
      "No commit carries a red Conflict badge",
      "No '1 conflicted commit' card or Resolve conflicts… button",
    ],
  });
  await expect(page.getByRole("button", { name: "Resolve conflicts…" })).toHaveCount(0);
});
