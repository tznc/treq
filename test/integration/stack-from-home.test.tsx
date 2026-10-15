// @include-parallel
import userEvent from "@testing-library/user-event";
import { invoke } from "@tauri-apps/api/core";
import * as React from "react";
import { beforeEach, describe, expect, it, vi } from "vitest";
import { Dashboard } from "../../src/components/Dashboard";
import { getWorkspaces } from "../../src/lib/api";
import { render, screen, waitFor, within } from "../test-utils";
import { createTestRepo, openRepo } from "../utils";
import { clickGpuiHome, findGpuiRow } from "../gpui-sidebar";

async function stackFromHome(
  user: ReturnType<typeof userEvent.setup>,
  branchName: string,
  onDialogOpen?: () => void,
) {
  await clickGpuiHome();
  await screen.findByTestId("show-workspace-header");
  await user.click(await screen.findByRole("button", { name: "Stack" }));
  const dialog = await screen.findByTestId("modal");
  onDialogOpen?.();
  await user.type(
    await within(dialog).findByLabelText("Branch Name"),
    branchName,
  );
  await user.click(
    within(dialog).getByRole("button", { name: "Create Workspace" }),
  );
  await waitFor(
    () => expect(screen.queryByTestId("modal")).not.toBeInTheDocument(),
    { timeout: 10_000 },
  );
  await waitFor(async () =>
    expect((await findGpuiRow(branchName)).selected).toBe(true),
  );
}

describe("Stack from the home repo", { timeout: 15_000 }, () => {
  let repoPath: string;
  let defaultBranch: string;
  let user: ReturnType<typeof userEvent.setup>;

  beforeEach(() => {
    ({ repoPath, defaultBranch } = createTestRepo(false));
    openRepo(repoPath);
    user = userEvent.setup();
  });

  it("creates only the requested workspace, not one for the home branch", async () => {
    render(<Dashboard />);

    await stackFromHome(user, "feat/first");

    const branches = (await getWorkspaces(repoPath)).map((w) => w.branch_name);
    expect(branches).toEqual(["feat/first"]);
    expect(branches).not.toContain(defaultBranch);
  });

  it("stacks on the home branch when Stack is clicked before that branch has loaded", async () => {
    const originalInvoke = vi.mocked(invoke).getMockImplementation();
    expect(originalInvoke).toBeTruthy();
    let releaseBranch: (() => void) | undefined;
    const branchGate = new Promise<void>((resolve) => {
      releaseBranch = resolve;
    });
    const toHold = new Set([
      "get_repo_current_branch",
      "get_repo_default_branch",
    ]);
    vi.mocked(invoke).mockImplementation(async (cmd, args) => {
      if (toHold.delete(cmd)) await branchGate;
      return originalInvoke!(cmd, args);
    });

    try {
      render(<Dashboard />);
      await stackFromHome(user, "feat/first", () => releaseBranch?.());
    } finally {
      releaseBranch?.();
      vi.mocked(invoke).mockImplementation(originalInvoke!);
    }

    const workspaces = await getWorkspaces(repoPath);
    expect(workspaces.map((w) => w.branch_name)).toEqual(["feat/first"]);
    expect(workspaces[0].target_branch).toBe(defaultBranch);
  });

  it("stacks a second workspace from home without an already-registered error", async () => {
    render(<Dashboard />);

    await stackFromHome(user, "feat/first");
    await stackFromHome(user, "feat/second");

    const branches = (await getWorkspaces(repoPath))
      .map((w) => w.branch_name)
      .sort();
    expect(branches).toEqual(["feat/first", "feat/second"]);
  }, 30_000);
});
