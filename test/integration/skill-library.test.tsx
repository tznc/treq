// @include-parallel
import * as React from "react";
import { beforeEach, describe, expect, it } from "vitest";
import fs from "fs";
import os from "os";
import path from "path";
import crypto from "crypto";
import { createTestRepo, openRepo, resolveWorkspacePath } from "../utils";
import { render, screen, waitFor, within } from "../test-utils";
import { Dashboard } from "../../src/components/Dashboard";
import userEvent from "@testing-library/user-event";
import { getWorkspaces, uninstallSkill } from "../../src/lib/api";

import { waitFor as waitForSnapshot } from "@testing-library/react";
import { gpuiSnapshot, sendGpuiAction } from "../gpui-sidebar";

async function openGpuiSettings() {
  await waitForSnapshot(() => expect(gpuiSnapshot()).toBeTruthy());
  sendGpuiAction({ kind: "openSettings" });
}

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
  const skillMd = Buffer.from(
    "---\nname: demo\ndescription: demo skill\n---\n# Demo\n",
  );
  const skillPath = path.join(dir, "SKILL.md");
  fs.writeFileSync(skillPath, skillMd);
  const checksum = skillChecksum([{ path: "SKILL.md", content: skillMd }]);
  const catalog = {
    generatedAt: new Date().toISOString(),
    sources: [{ id: "test", name: "Test", url: "https://example.test" }],
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
  };
  const catalogPath = path.join(dir, "catalog.json");
  fs.writeFileSync(catalogPath, JSON.stringify(catalog));
  process.env.TREQ_SKILLS_CATALOG_URL = catalogPath;
  return catalogPath;
}

describe("skill library", () => {
  let user: ReturnType<typeof userEvent.setup>;

  beforeEach(async () => {
    const { repoPath } = createTestRepo(false);
    openRepo(repoPath);
    writeCatalog();
    await uninstallSkill("test/demo", repoPath).catch(() => undefined);
    user = userEvent.setup();
  });

  async function openSkillsTab() {
    render(<Dashboard />);
    await openGpuiSettings();
    await user.click(await screen.findByRole("tab", { name: /skills/i }));
    expect(await screen.findByTestId("skill-library-settings")).toBeTruthy();
  }

  it("installs from a dialog that offers repository as the primary action", async () => {
    await openSkillsTab();
    expect(await screen.findByText("demo skill")).toBeTruthy();

    await user.click(await screen.findByRole("button", { name: /^install/i }));
    const dialog = await screen.findByRole("dialog");
    expect(dialog).toHaveTextContent(/repository/i);
    expect(dialog).toHaveTextContent(/application/i);

    await user.click(
      await screen.findByRole("button", { name: /install for application/i }),
    );

    expect(await screen.findByLabelText(/install level for demo/i)).toHaveValue(
      "application",
    );
    expect(
      await screen.findByRole("button", { name: /uninstall demo/i }),
    ).toBeTruthy();
  }, 30000);

  it("filters the catalog by install level", async () => {
    await openSkillsTab();
    await user.click(await screen.findByRole("button", { name: /^install/i }));
    await user.click(
      await screen.findByRole("button", { name: /install for repository/i }),
    );
    expect(await screen.findByLabelText(/install level for demo/i)).toHaveValue(
      "repository",
    );

    await user.selectOptions(
      await screen.findByLabelText(/filter by install level/i),
      "application",
    );
    expect(screen.queryByText("demo skill")).toBeNull();

    await user.selectOptions(
      await screen.findByLabelText(/filter by install level/i),
      "repository",
    );
    expect(await screen.findByText("demo skill")).toBeTruthy();
  }, 30000);

  it("materializes installed library skills when creating a stacked workspace", async () => {
    const BRANCH_NAME = "feat/skill-materialize";
    await openSkillsTab();
    await user.click(await screen.findByRole("button", { name: /^install/i }));
    await user.click(
      await screen.findByRole("button", { name: /install for repository/i }),
    );
    await user.click(await screen.findByRole("button", { name: /^close$/i }));

    await screen.findByTestId("show-workspace-header");
    await user.keyboard("{Control>}k{/Control}");
    await user.click(await screen.findByText("Create Workspace"));
    const dialog = await screen.findByTestId("modal");
    await user.type(
      await within(dialog).findByLabelText("Branch Name"),
      BRANCH_NAME,
    );
    await user.click(
      within(dialog).getByRole("button", { name: "Create Workspace" }),
    );
    await waitFor(() => {
      expect(screen.queryByTestId("modal")).not.toBeInTheDocument();
    });

    const repoPath = new URL(window.location.href).searchParams.get("repo");
    if (!repoPath) throw new Error("repo path missing from URL");
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
    expect(
      fs.existsSync(path.join(workspacePath, ".agents/skills/demo/SKILL.md")),
    ).toBe(true);
    expect(
      fs.existsSync(path.join(workspacePath, ".claude/skills/demo/SKILL.md")),
    ).toBe(true);
  }, 30000);
});
