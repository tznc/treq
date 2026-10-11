// @include-parallel
import * as React from "react";
import { execSync } from "node:child_process";
import { beforeEach, describe, expect, it } from "vitest";
import userEvent from "@testing-library/user-event";
import { render, screen } from "../test-utils";
import { createTestRepo, gitCommitRepoFile, openRepo } from "../utils";
import { Dashboard } from "../../src/components/Dashboard";

describe("Home repo - Commits tab", () => {
  let repoPath: string;
  let defaultBranch: string;
  let user: ReturnType<typeof userEvent.setup>;

  beforeEach(async () => {
    ({ repoPath, defaultBranch } = createTestRepo(false));
    openRepo(repoPath);
    user = userEvent.setup();
    await gitCommitRepoFile(
      repoPath,
      "main-history.txt",
      "main history",
      "Main history commit",
    );
  });

  it("shows default branch history below the branch's commits on a non-default branch", async () => {
    execSync("git checkout -b feat/home-branch", { cwd: repoPath });
    await gitCommitRepoFile(
      repoPath,
      "home-branch.txt",
      "home branch",
      "Home branch commit",
    );

    render(<Dashboard />);
    await user.click(await screen.findByRole("tab", { name: /^Commits/ }));

    await screen.findByText("Home branch commit");
    await screen.findByText(`Recent on ${defaultBranch}`);
    await screen.findByText("Main history commit");
    expect(screen.getAllByText("Home branch commit")).toHaveLength(1);
  });

  it("does not show a default branch section when on the default branch", async () => {
    render(<Dashboard />);
    await user.click(await screen.findByRole("tab", { name: /^Commits/ }));

    await screen.findByText("Main history commit");
    expect(screen.queryByText(/^Recent on /)).not.toBeInTheDocument();
  });
});
