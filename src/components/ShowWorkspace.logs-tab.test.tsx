import { screen } from "@testing-library/react";
import { beforeEach, describe, expect, it, vi } from "vitest";
import { render } from "../../test/test-utils";
import type { Workspace } from "../lib/api";
import { ShowWorkspace } from "./ShowWorkspace";

vi.mock("../lib/api", async () => {
  const actual =
    await vi.importActual<typeof import("../lib/api")>("../lib/api");
  return {
    ...actual,
    getWorkspaceStatus: vi.fn().mockResolvedValue({
      conflicted_files: [],
      remote_sync: { type: "Synced" },
    }),
    listCommits: vi.fn().mockResolvedValue({ commits: [] }),
    lsWorkspaceWithStatus: vi
      .fn()
      .mockImplementation(() => new Promise(() => {})),
    getWorkspaceReadme: vi.fn().mockImplementation(() => new Promise(() => {})),
  };
});

vi.mock("./LinearCommitHistory", () => ({ LinearCommitHistory: () => null }));
vi.mock("../stores/featurePreviewStore", async () => ({
  ...(await vi.importActual<object>("../stores/featurePreviewStore")),
  usePreviewFeature: () => true,
}));

const workspace: Workspace = {
  id: 2,
  repo_path: "/repo",
  workspace_name: "workspace-2",
  workspace_path: "/repo/.treq/workspaces/workspace-2",
  branch_name: "feature-2",
  title: "Feature two",
  created_at: new Date().toISOString(),
  not_on_remote: false,
};

describe("ShowWorkspace logs tab", () => {
  beforeEach(() => vi.clearAllMocks());

  it("hides the Logs tab for workspaces", () => {
    render(
      <ShowWorkspace
        repositoryPath="/repo"
        workspace={workspace}
        mainRepoBranch="main"
        initialSelectedFile={null}
      />,
    );
    expect(screen.queryByRole("tab", { name: /logs/i })).toBeNull();
  });

  it("shows the Logs tab for the home repo", () => {
    render(
      <ShowWorkspace
        repositoryPath="/repo"
        workspace={null}
        mainRepoBranch="main"
        initialSelectedFile={null}
      />,
    );
    expect(screen.getByRole("tab", { name: /logs/i })).toBeTruthy();
  });
});
