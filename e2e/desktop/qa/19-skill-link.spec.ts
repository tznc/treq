// Skills from treq.dev (TREQ-297) and customizing them (TREQ-298): a
// `treq://skill/install` link, as the website's "Open in Treq" button sends
// it, opens Settings on the Skills tab with a confirmation listing the
// catalog entry's description, metadata, checksum and registry link.
// Installing puts the skill in application data for every agent; Customize
// writes a user-owned copy into the repository and starts an agent on it.
//
// The link reaches the running app the way the OS delivers it on Linux: a
// second `treq <url>` process, which the single-instance plugin hands to the
// first over D-Bus (run the QA under `dbus-run-session`). The skill is a
// real one from the live catalog, so the run needs network access. The app
// finds agents on the login shell's PATH: where that shell's profile puts a
// real `claude` ahead of the fixture's, point SHELL at a shell that skips
// profiles (`exec bash --noprofile --norc "$@"`).
//
// `QA_MARKS` (a file path) records when each step starts, for captioning a
// screen recording of the run.
import { execFileSync, spawn } from "node:child_process";
import { appendFileSync, copyFileSync, writeFileSync } from "node:fs";
import { expect, test, type Page } from "@playwright/test";
import { appBinary } from "../env.ts";
import { appEnv } from "../harness.ts";
import { agentCalls, fixture } from "../fixture.ts";
import { capture } from "./capture.ts";
import { home, settle } from "./helpers.ts";

const SKILL_ID = "anthropic/frontend-design";
const SKILL = "frontend-design";

test.describe.configure({ mode: "serial" });

function mark(label: string): void {
  const file = process.env.QA_MARKS;
  if (file) appendFileSync(file, `${Date.now()}\t${label}\n`);
}

/** Delivers `url` to the running app as a second process would. */
function openLink(url: string): void {
  const f = fixture();
  const child = spawn(appBinary, [url], {
    env: appEnv({ ...f, bin: `${f.root}/bin` }),
    stdio: "ignore",
    detached: true,
  });
  child.unref();
}

async function skillsTab(page: Page): Promise<void> {
  const close = page.getByRole("button", { name: "Close", exact: true });
  if ((await close.count()) === 0) {
    await page.getByRole("button", { name: "Settings", exact: true }).click();
    await expect(close).toBeVisible();
  }
  await page.getByRole("tab", { name: "Skills" }).click();
  await page.mouse.move(1000, 860);
  await settle(page, 1200);
}

test("skill link: confirm, install, customize with an agent", async ({ page }) => {
  await home(page);
  mark("Treq is open on a repository");
  await settle(page, 1500);

  mark("The website's Open in Treq button sends treq://skill/install?id=anthropic/frontend-design");
  openLink(`treq://skill/install?id=${encodeURIComponent(SKILL_ID)}`);
  // The dialog's text is not observable; its buttons are.
  const registry = page.getByRole("button", { name: "View in skill registry" });
  await expect(registry).toBeVisible({ timeout: 60_000 });
  await expect(page.getByRole("button", { name: "Install", exact: true })).toBeVisible();
  mark("Treq asks first: description, catalog metadata, SHA-256 and a registry link");
  await settle(page, 2500);
  await capture(page, {
    name: "19-skill-link-confirm",
    title: "A treq://skill/install link's confirmation",
    expectations: [
      "A dialog 'Install frontend-design for all agents?' over the Skills tab, with the skill's description",
      "A bordered list: catalog id, source, license, files, then the full SHA-256 checksum",
      "'View in skill registry' on the left; Cancel and Install on the right",
    ],
  });

  mark("Install: the catalog copy downloads and is checked against the checksum");
  await page.getByRole("button", { name: "Install", exact: true }).click();
  await expect(registry).toHaveCount(0, { timeout: 60_000 });
  const customize = page.getByRole("button", { name: `Customize ${SKILL}` });
  await expect(customize).toBeVisible({ timeout: 60_000 });
  await settle(page, 2000);
  mark("Installed for all agents: it lists under Installed, App-wide, with Customize");
  await settle(page, 1500);
  await capture(page, {
    name: "19-skill-link-installed",
    title: "The linked skill, installed at the application level",
    expectations: [
      "An Installed heading with App-wide (frontend-design, Install level Application) and an empty Local group",
      "The frontend-design row has Customize on the left and a trash icon top right",
      "A Skill registry heading follows, listing skills that are not installed",
    ],
  });

  // The QA agent prints a fixed transcript; for this step it shows the
  // prompt it was started with instead.
  const agent = `${fixture().root}/bin/claude`;
  copyFileSync(agent, `${agent}.qa`);
  writeFileSync(
    agent,
    `#!/bin/sh\nprintf '%s\\n' "$*" >> '${fixture().agentLog}'\nprintf '\\033[1mfake agent started with:\\033[0m\\n%s\\n' "$*" | fold -s -w 110\nexec cat\n`,
  );
  mark("Customize: Treq writes a user-owned copy and starts an agent on it");
  try {
    await customize.click();
    await expect(page.getByRole("button", { name: "Close", exact: true })).toHaveCount(0, {
      timeout: 30_000,
    });
    await expect
      .poll(() => agentCalls().some((call) => call.includes(".agents/skills/frontend-design/")))
      .toBe(true);
    await settle(page, 4000);
  } finally {
    copyFileSync(`${agent}.qa`, agent);
  }
  mark("The agent session opens on .agents/skills/frontend-design");
  await settle(page, 2000);
  await capture(page, {
    name: "19-skill-link-agent",
    title: "An agent session started on the customized copy",
    expectations: [
      "The workspace view with a terminal pane running the agent",
      "The terminal shows the prompt: customize frontend-design at .agents/skills/frontend-design/ and .claude/skills/frontend-design/",
    ],
  });

  await skillsTab(page);
  await expect(page.getByRole("button", { name: `Edit ${SKILL} with an agent` })).toBeVisible();
  mark("Customized: it moves to Local with a badge; catalog updates never overwrite the copy");
  await settle(page, 2500);
  await capture(page, {
    name: "19-skill-link-customized",
    title: "The customized skill in the Skills tab",
    expectations: [
      "App-wide is empty; frontend-design now lists under Local",
      "Its row has a 'Customized' badge and an 'Edit with agent' button",
    ],
  });
  const { repo } = fixture();
  execFileSync("test", ["-f", `${repo}/.agents/skills/${SKILL}/SKILL.md`]);
  execFileSync("test", ["!", "-e", `${repo}/.agents/skills/${SKILL}/.treq-generated`]);
  mark("end");
});
