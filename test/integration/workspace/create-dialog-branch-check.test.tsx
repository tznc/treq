// @include-parallel
import { clickGpuiHome, findGpuiRow } from "../../gpui-sidebar";
import userEvent from "@testing-library/user-event";
import * as React from "react";
import { beforeEach, describe, expect, it } from "vitest";
import { Dashboard } from "../../../src/components/Dashboard";
import {
  archiveWorkspace,
  createWorkspace,
  pushWorkspaceToRemote,
} from "../../../src/lib/api";
import { render, screen, waitFor, within } from "../../test-utils";
import {
  commitWorkspaceFile,
  createTestRepo,
  openRepo,
  resolveRevsetCommitIds,
} from "../../utils";

describe("create dialog branch check", () => {
  let repoPath: string;
  let user: ReturnType<typeof userEvent.setup>;

  beforeEach(() => {
    ({ repoPath } = createTestRepo(true));
    openRepo(repoPath);
    user = userEvent.setup();
  });

  it("does not start a new branch from the previous name's remote branch", async () => {
    const featA = await createWorkspace(repoPath, "feat-a");
    await commitWorkspaceFile(
      repoPath,
      { id: featA, path: "feat-a" },
      "a.txt",
      "only on feat-a",
      "feat-a work",
    );
    await pushWorkspaceToRemote(repoPath, featA);
    await archiveWorkspace(repoPath, featA);

    render(<Dashboard />);
    await clickGpuiHome();
    await screen.findByTestId("show-workspace-header");
    await user.click(await screen.findByRole("button", { name: "Stack" }));
    const dialog = await screen.findByTestId("modal");
    const input = await within(dialog).findByLabelText("Branch Name");

    await user.type(input, "feat-a");
    await within(dialog).findByText("Branch already exists locally");

    await user.clear(input);
    await user.type(input, "feat-b");
    await user.click(
      within(dialog).getByRole("button", { name: "Create Workspace" }),
    );
    await waitFor(() =>
      expect(screen.queryByTestId("modal")).not.toBeInTheDocument(),
    );
    await findGpuiRow("feat-b");
    await waitFor(() =>
      expect(screen.getByTestId("show-workspace-header")).toHaveTextContent(
        "feat-b",
      ),
    );

    expect(resolveRevsetCommitIds(repoPath, "feat-a & ::feat-b")).toEqual([]);
  });
});
