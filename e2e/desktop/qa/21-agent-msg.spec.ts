// `treq agent msg`: one agent messages another through the CLI. The fake
// `claude` leaves text on its prompt line, so treq reads it as a draft and
// types nothing: the target's header shows a count and its terminal a
// banner. "Deliver" types the wake line.
import { execFileSync } from "node:child_process";
import { readdirSync, readFileSync, statSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { expect, test } from "@playwright/test";
import { appBinary } from "../env.ts";
import { capture } from "./capture.ts";
import { openTab, openWorkspace, row, settle } from "./helpers.ts";

const SENDER = "feat/empty-event-message";
const TARGET = "feat/keyvalues-cache";

test.describe.configure({ mode: "serial" });

/** The running QA app's demo repository. */
function repoDir(): string {
  const roots = readdirSync(tmpdir())
    .filter((d) => d.startsWith("treq-qa-"))
    .map((d) => join(tmpdir(), d))
    .filter((d) => {
      try {
        return statSync(join(d, "qa-extra.json")).isFile();
      } catch {
        return false;
      }
    })
    .sort((a, b) => statSync(b).mtimeMs - statSync(a).mtimeMs);
  return JSON.parse(readFileSync(join(roots[0], "qa-extra.json"), "utf8")).repo;
}

type Agent = { handle: string; pty_id: string; workspace: string | null };

/** Runs `treq agent msg` in the repo, as the terminal `pty` when given. */
function msg(repo: string, args: string[], pty?: string): string {
  return execFileSync(appBinary, ["agent", "msg", ...args], {
    cwd: repo,
    env: { ...process.env, ...(pty ? { TREQ_PTY_SESSION_ID: pty } : {}) },
  }).toString();
}

test("agent msg: a waiting message shows a badge and a banner", async ({ page }) => {
  const repo = repoDir();
  for (const branch of [SENDER, TARGET]) {
    await openWorkspace(page, branch);
    await row(page, branch).hover();
    await row(page, branch).getByRole("button", { name: "Start agent" }).click();
    await settle(page, 3000);
  }
  await openTab(page, "Commits");

  const agents: Agent[] = JSON.parse(msg(repo, ["--list", "--json"])).agents;
  const sender = agents.find((a) => a.workspace === SENDER);
  const target = agents.find((a) => a.workspace === TARGET);
  expect(sender, JSON.stringify(agents)).toBeTruthy();
  expect(target, JSON.stringify(agents)).toBeTruthy();

  const sent = JSON.parse(msg(repo, [target!.handle, "rebase done?", "--json"], sender!.pty_id));
  expect(sent.targets[0].outcome).toBe("queued");
  await settle(page, 2500);

  await expect(page.getByRole("button", { name: "Deliver" })).toBeVisible();
  await expect(page.getByRole("button", { name: "Show message" })).toHaveCount(0);
  await capture(page, {
    name: "21-agent-msg-banner",
    title: "A message waits while the prompt holds text",
    expectations: [
      "The target agent's terminal shows an amber banner naming the sender, with Deliver at its right",
      "The target's column header shows a count of 1 beside its search and close buttons",
      "Nothing was typed into the target's prompt",
    ],
  });

  await page.getByRole("button", { name: "Deliver" }).click();
  await settle(page, 1500);
  await capture(page, {
    name: "21-agent-msg-delivered",
    title: "Deliver types the wake line",
    expectations: [
      "The target terminal shows a [treq] line naming the sender and treq agent msg --inbox",
      "The message text itself is not typed into the terminal",
    ],
  });
});
