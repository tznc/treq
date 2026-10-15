import * as React from "react";
import fs from "node:fs";
import os from "node:os";
import path from "node:path";
import userEvent from "@testing-library/user-event";
import { it, onTestFinished } from "vitest";
import { Dashboard } from "../../../src/components/Dashboard";
import { render, screen, within } from "../../../test/test-utils";
import { createTestRepo, openRepo } from "../../../test/utils";
import { captureDocument } from "../capture";
import { sendGpuiAction } from "../../../test/gpui-sidebar";

const FAKE_TOOL = path.resolve(
  __dirname,
  "../../../test/fake-agent/fake-agent.sh",
);

/**
 * Points the backend's tool check at a directory holding only `tools` (plus a
 * link to the system `which`, which the lookup runs from the same PATH).
 */
function useSearchPathWith(tools: string[]) {
  const searchDir = fs.mkdtempSync(path.join(os.tmpdir(), "treq-prereq-"));
  const which = (process.env.PATH ?? "")
    .split(path.delimiter)
    .map((dir) => path.join(dir, "which"))
    .find((candidate) => fs.existsSync(candidate));
  if (!which) throw new Error("`which` must be on PATH");
  fs.symlinkSync(which, path.join(searchDir, "which"));
  for (const tool of tools) {
    fs.symlinkSync(FAKE_TOOL, path.join(searchDir, tool));
  }
  process.env.TREQ_TEST_PREREQ_PATH = searchDir;
  onTestFinished(() => {
    delete process.env.TREQ_TEST_PREREQ_PATH;
    fs.rmSync(searchDir, { recursive: true, force: true });
  });
}

const findChecklist = () =>
  screen.findByRole("region", { name: "Setup checklist" });

it("captures the setup checklist with Git and one agent installed", async () => {
  useSearchPathWith(["git", "claude"]);
  window.history.replaceState({}, "", "/");
  render(<Dashboard />);

  const checklist = await findChecklist();
  await within(
    await within(checklist).findByRole("listitem", { name: "Git" }),
  ).findByText("Installed");
  await within(checklist).findByRole("button", { name: "Install Codex" });

  await captureDocument(document, {
    name: "onboarding-prerequisites-01-checklist",
    expectations: [
      "Below the 'Open Repository' button, a 'Before you start' list shows Git and Claude Code with green checks and 'Installed'.",
      "Codex, Cursor Agent, GitHub Copilot CLI and GitHub CLI each show an 'Install' link instead of 'Installed'.",
      "The rows sit under 'Required', 'Agent CLI (install at least one)' and 'Optional' headings, and GitHub CLI notes 'Needed for pull requests and issues'.",
    ],
  });
});

it("captures the tool checklist on the Application settings tab", async () => {
  useSearchPathWith(["git", "codex", "gh"]);
  const { repoPath } = createTestRepo(false);
  openRepo(repoPath);
  const user = userEvent.setup();
  render(<Dashboard />);

  sendGpuiAction({ kind: "openSettings" });
  await user.click(await screen.findByRole("tab", { name: /application/i }));
  const checklist = await findChecklist();
  await within(
    await within(checklist).findByRole("listitem", { name: "Codex" }),
  ).findByText("Installed");

  await captureDocument(document, {
    name: "onboarding-prerequisites-03-settings",
    viewport: { width: 1440, height: 1100 },
    expectations: [
      "The Application settings tab shows a 'Command-line tools' checklist below the Default Agent picker.",
      "Git, Codex and GitHub CLI show green checks with 'Installed'; the other agents show 'Install' links.",
    ],
  });
});

it("captures the setup checklist with nothing installed", async () => {
  useSearchPathWith([]);
  window.history.replaceState({}, "", "/");
  render(<Dashboard />);

  const checklist = await findChecklist();
  await within(checklist).findByRole("button", { name: "Install Git" });

  await captureDocument(document, {
    name: "onboarding-prerequisites-02-nothing-installed",
    expectations: [
      "Git shows a red X icon and an 'Install' link.",
      "The 'Agent CLI (install at least one)' heading is red, and every agent row shows an 'Install' link.",
    ],
  });
});
