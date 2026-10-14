// @include-parallel
import * as React from "react";
import userEvent from "@testing-library/user-event";
import { beforeEach, describe, expect, it } from "vitest";
import { Dashboard } from "../../src/components/Dashboard";
import {
  createWorkspace,
  getRepoSetting,
  getSessions,
  getWorkspaces,
  setRepoSetting,
} from "../../src/lib/api";
import { render, screen, waitFor } from "../test-utils";
import { createTestRepo, findSidebarBranchElement, openRepo } from "../utils";

describe("home-view task input", () => {
  let user: ReturnType<typeof userEvent.setup>;
  let repoPath: string;

  const submitTask = async (text: string) => {
    await user.type(
      await screen.findByPlaceholderText("Describe a task..."),
      text,
    );
    await user.click(await screen.findByRole("button", { name: /^edit$/i }));
  };

  const findToggle = () =>
    screen.findByRole("switch", { name: "Run in a new workspace" });

  beforeEach(() => {
    ({ repoPath } = createTestRepo(false));
    openRepo(repoPath);
    user = userEvent.setup();
  });

  it("starts the task in a new workspace named after it by default", async () => {
    render(<Dashboard />);
    expect(await findToggle()).toBeChecked();

    await submitTask("Fix the login redirect");

    expect(
      await findSidebarBranchElement("treq/fix-the-login-redirect"),
    ).toBeTruthy();
    await waitFor(async () => {
      const workspace = (await getWorkspaces(repoPath)).find(
        (ws) => ws.branch_name === "treq/fix-the-login-redirect",
      );
      const sessions = await getSessions(repoPath);
      expect(workspace).toBeDefined();
      expect(sessions.map((s) => s.workspace_id)).toEqual([workspace?.id]);
    });
  });

  it("applies the configured branch pattern and avoids prefixed name collisions", async () => {
    await setRepoSetting(repoPath, "branch_name_pattern", "team/{name}");
    await createWorkspace(repoPath, "team/fix-bug");
    await createWorkspace(repoPath, "team/fix-bug-2");
    render(<Dashboard />);
    await findToggle();

    await submitTask("Fix bug");

    expect(await findSidebarBranchElement("team/fix-bug-3")).toBeTruthy();
    await waitFor(async () => {
      const workspace = (await getWorkspaces(repoPath)).find(
        (ws) => ws.branch_name === "team/fix-bug-3",
      );
      expect(workspace).toBeDefined();
      expect((await getSessions(repoPath)).map((s) => s.workspace_id)).toEqual([
        workspace?.id,
      ]);
    });
  });

  it("runs the task in the home repo when the toggle is off, and remembers it", async () => {
    const { unmount } = render(<Dashboard />);
    await user.click(await findToggle());
    expect(await findToggle()).not.toBeChecked();
    await waitFor(async () =>
      expect(await getRepoSetting(repoPath, "home_task_new_workspace")).toBe(
        "false",
      ),
    );

    await submitTask("Fix the login redirect");

    await waitFor(async () => {
      const sessions = await getSessions(repoPath);
      expect(sessions.map((s) => s.workspace_id)).toEqual([null]);
    });
    expect(await getWorkspaces(repoPath)).toEqual([]);

    unmount();
    render(<Dashboard />);
    expect(await findToggle()).not.toBeChecked();
  });

  it("does not offer the toggle inside a workspace", async () => {
    await createWorkspace(repoPath, "feat/existing");
    render(<Dashboard />);
    await findToggle();

    await user.click(await findSidebarBranchElement("feat/existing"));

    await screen.findByPlaceholderText("Describe a task...");
    await waitFor(() =>
      expect(
        screen.queryByRole("switch", { name: "Run in a new workspace" }),
      ).not.toBeInTheDocument(),
    );
  });
});
