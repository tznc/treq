import * as React from "react";
import { expect, it } from "vitest";
import fs from "fs";
import os from "os";
import path from "path";
import crypto from "crypto";
import userEvent from "@testing-library/user-event";
import {
  createTestRepo,
  openRepo,
  resolveWorkspacePath,
} from "../../../test/utils";
import { render, screen, waitFor, within } from "../../../test/test-utils";
import { Dashboard } from "../../../src/components/Dashboard";
import { captureDocument } from "../capture";
import { getWorkspaces, readFile, uninstallSkill } from "../../../src/lib/api";
import { findGpuiRow, sendGpuiAction, startGpuiAgent } from "../../../test/gpui-sidebar";

const BRANCH_NAME = "feat/with-demo-skill";
const SKILL_BODY = "---\nname: demo\ndescription: demo skill\n---\n# Demo\n";

function skillChecksum(files: { path: string; content: Buffer }[]): string {
  const hash = crypto.createHash("sha256");
  for (const file of [...files].sort((a, b) => a.path.localeCompare(b.path))) {
    hash.update(file.path);
    hash.update(Buffer.from([0]));
    const len = Buffer.alloc(8);
    len.writeBigUInt64BE(BigInt(file.content.length));
    hash.update(len);
    hash.update(file.content);
  }
  return hash.digest("hex");
}

function writeCatalog() {
  const dir = fs.mkdtempSync(path.join(os.tmpdir(), "treq-skill-catalog-"));
  const skillMd = Buffer.from(SKILL_BODY);
  const skillPath = path.join(dir, "SKILL.md");
  fs.writeFileSync(skillPath, skillMd);
  const checksum = skillChecksum([{ path: "SKILL.md", content: skillMd }]);
  const catalogPath = path.join(dir, "catalog.json");
  fs.writeFileSync(
    catalogPath,
    JSON.stringify({
      generatedAt: new Date().toISOString(),
      sources: [{ id: "test", name: "Test" }],
      skills: [
        {
          id: "test/demo",
          name: "demo",
          description: "demo skill",
          source: "test",
          proprietary: false,
          checksum,
          files: [
            {
              path: "SKILL.md",
              size: skillMd.length,
              binary: false,
              rawUrl: `file://${skillPath}`,
            },
          ],
        },
      ],
    }),
  );
  process.env.TREQ_SKILLS_CATALOG_URL = catalogPath;
}

it("materializes installed library skills into a new workspace", async () => {
  const { repoPath } = createTestRepo(false);
  openRepo(repoPath);
  writeCatalog();
  await uninstallSkill("test/demo", repoPath).catch(() => undefined);

  const user = userEvent.setup();
  render(<Dashboard />);

  sendGpuiAction({ kind: "openSettings" });
  await user.click(await screen.findByRole("tab", { name: /skills/i }));
  await user.click(await screen.findByRole("button", { name: /^install/i }));
  await user.click(
    await screen.findByRole("button", { name: /install for repository/i }),
  );
  expect(await screen.findByText("1 installed")).toBeTruthy();
  await user.click(await screen.findByRole("button", { name: /^close$/i }));

  await screen.findByTestId("show-workspace-header");
  await user.click(await screen.findByRole("button", { name: "Stack" }));
  const dialog = await screen.findByTestId("modal");
  await user.type(
    within(dialog).getByLabelText("Branch Name"),
    BRANCH_NAME,
  );
  await user.click(
    within(dialog).getByRole("button", { name: "Create Workspace" }),
  );
  await waitFor(() => {
    expect(screen.queryByTestId("modal")).not.toBeInTheDocument();
  });

  const header = await screen.findByTestId("show-workspace-header");
  await within(header).findByText(BRANCH_NAME);
  await findGpuiRow(BRANCH_NAME);

  const workspace = (await getWorkspaces(repoPath)).find(
    (candidate) => candidate.branch_name === BRANCH_NAME,
  );
  if (!workspace) {
    throw new Error(`Expected ${BRANCH_NAME} workspace to exist`);
  }
  const workspacePath = resolveWorkspacePath(
    repoPath,
    workspace.workspace_path,
  );

  const agentsSkill = path.join(workspacePath, ".agents/skills/demo/SKILL.md");
  const claudeSkill = path.join(workspacePath, ".claude/skills/demo/SKILL.md");
  expect(fs.existsSync(agentsSkill)).toBe(true);
  expect(fs.existsSync(claudeSkill)).toBe(true);
  expect(
    fs.existsSync(path.join(workspacePath, ".agents/skills/demo/.treq-generated")),
  ).toBe(true);
  expect(
    fs.existsSync(path.join(workspacePath, ".claude/skills/demo/.treq-generated")),
  ).toBe(true);
  expect(await readFile(agentsSkill)).toContain("# Demo");
  expect(await readFile(claudeSkill)).toContain("# Demo");

  await captureDocument(document, {
    name: "skill-workspace-01-created",
    expectations: [
      "The new workspace branch feat/with-demo-skill is selected in the sidebar and workspace header.",
      "The workspace overview is visible after closing Settings and stacking from the home repo.",
    ],
  });

  await startGpuiAgent(BRANCH_NAME);
  await waitFor(() => {
    expect(
      document.querySelector('[data-terminal-id^="agent-"]'),
    ).not.toBeNull();
  });

  await captureDocument(document, {
    name: "skill-workspace-02-agent-ready",
    expectations: [
      "An agent terminal session is open on the feat/with-demo-skill workspace.",
      "The workspace remains selected in the sidebar with the terminal pane visible.",
    ],
  });
}, 90000);
