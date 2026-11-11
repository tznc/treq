// `treq agent start` with a long prompt, as one agent hands a
// task to another. treq types the agent's command into a shell that is
// still starting, whose line discipline caps typed input (a 4095-byte
// line on Linux, about 1 KB in all on macOS) and drops the rest. The agent must start on its own and get the
// whole prompt.
import { execFileSync } from "node:child_process";
import { existsSync, readdirSync, readFileSync, rmSync, statSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { expect, test } from "@playwright/test";
import { appBinary } from "../env.ts";
import { capture } from "./capture.ts";
import { ready, row, settle } from "./helpers.ts";

const BRANCH = "feat/alerting-logs";

/** The running QA app's fixture root. */
function fixtureRoot(): string {
  return readdirSync(tmpdir())
    .filter((d) => d.startsWith("treq-qa-"))
    .map((d) => join(tmpdir(), d))
    .filter((d) => {
      try {
        return statSync(join(d, "qa-extra.json")).isFile();
      } catch {
        return false;
      }
    })
    .sort((a, b) => statSync(b).mtimeMs - statSync(a).mtimeMs)[0];
}

test("agent start: a long prompt reaches the agent without an Enter", async ({ page }) => {
  await ready(page);
  const root = fixtureRoot();
  const repo = JSON.parse(readFileSync(join(root, "qa-extra.json"), "utf8")).repo;
  const log = join(root, "agent-calls.log");
  const before = existsSync(log) ? readFileSync(log, "utf8") : "";

  const steps = Array.from(
    { length: 60 },
    (_, i) => `${i + 1}. Check alert rule ${i + 1}: level, channel and the retry it's sent with.`,
  );
  // One line longer than Linux's 4095-byte line: cut, the typed command
  // loses its closing quote and the shell waits for more.
  const prompt = `Audit the alerting pipeline. ${steps.join(" ")} END-OF-TASK`;
  expect(prompt.length).toBeGreaterThan(4096);

  writeFileSync(join(root, "slow-shell"), "");
  try {
    // The app registers its window a heartbeat after it shows the repo.
    await expect
      .poll(
        () => {
          try {
            execFileSync(appBinary, ["agent", "start", BRANCH, prompt], {
              cwd: repo,
              env: { ...process.env, HOME: join(root, "home"), PATH: `${join(root, "bin")}:${process.env.PATH}` },
              stdio: "pipe",
            });
            return true;
          } catch {
            return false;
          }
        },
        { timeout: 20_000, intervals: [1000] },
      )
      .toBe(true);
    // Nothing is pressed: the agent has to start by itself.
    await expect
      .poll(() => (existsSync(log) ? readFileSync(log, "utf8").slice(before.length) : ""), {
        timeout: 20_000,
      })
      .toContain("END-OF-TASK");
  } finally {
    rmSync(join(root, "slow-shell"), { force: true });
  }
  const call = readFileSync(log, "utf8").slice(before.length);
  expect(call).toContain(prompt);
  expect(call).toContain("--append-system-prompt-file");

  await expect(row(page, BRANCH)).toBeVisible();
  await settle(page, 1500);
  await capture(page, {
    name: "29-agent-start-long-prompt",
    title: "An agent started by `treq agent start` with a 5 KB prompt",
    expectations: [
      "The workspace feat/alerting-logs is selected with an agent column open in the terminal pane",
      "The fake agent's transcript shows: the agent ran without anyone pressing Enter",
      "No shell continuation prompt (`>`) or half-typed command is left in the column",
    ],
  });
});
