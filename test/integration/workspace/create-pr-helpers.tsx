import type userEvent from "@testing-library/user-event";
import { clickGpuiRow } from "../../gpui-sidebar";
import { expect } from "vitest";
import {
  createWorkspace,
  getWorkspaces,
  pushWorkspaceToRemote,
  updateWorkspace,
} from "../../../src/lib/api";
import { screen, waitFor, within } from "../../test-utils";
import { commitWorkspaceFile, setOriginUrl } from "../../utils";

export async function setupPushedWorkspaceWithGitHub(
  repoPath: string,
  options?: {
    title?: string;
    description?: string;
    githubRemote?: boolean;
  },
) {
  const title = options?.title ?? "Ship the feature";
  const description =
    options?.description ?? "Implements the feature end-to-end.";
  const workspaceId = await createWorkspace(repoPath, "feat/create-pr");
  await updateWorkspace(repoPath, workspaceId, undefined, title, description);
  const created = (await getWorkspaces(repoPath)).find(
    (w) => w.id === workspaceId,
  )!;
  await commitWorkspaceFile(
    repoPath,
    { id: created.id, path: created.workspace_path },
    "feature.txt",
    "feature content",
    "Add feature",
  );
  await pushWorkspaceToRemote(repoPath, workspaceId);

  if (options?.githubRemote !== false) {
    setOriginUrl(repoPath, "https://github.com/acme/treq.git");
  }

  const workspace = (await getWorkspaces(repoPath)).find(
    (w) => w.branch_name === "feat/create-pr",
  );
  expect(workspace?.not_on_remote).toBe(false);
  return { workspace: workspace!, title, description };
}

export async function openWorkspace(
  user: ReturnType<typeof userEvent.setup>,
  branchName: string,
) {
  await clickGpuiRow(branchName);
  return screen.findByTestId("show-workspace-header");
}

export async function findEnabledCreatePr(header: HTMLElement) {
  const createPr = await within(header).findByRole("button", {
    name: /^create pr$/i,
  });
  await waitFor(() => {
    expect(createPr).toBeEnabled();
  });
  return createPr;
}
