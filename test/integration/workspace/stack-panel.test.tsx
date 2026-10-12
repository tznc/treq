// @include-parallel
import userEvent from "@testing-library/user-event";
import { beforeEach, describe, expect, it } from "vitest";
import { Dashboard } from "../../../src/components/Dashboard";
import {
  createWorkspace,
  getWorkspaces,
  setWorkspaceTargetBranch,
} from "../../../src/lib/api";
import { getFullWorkspacePath } from "../../../src/lib/utils";
import { render, screen, waitFor, within } from "../../test-utils";
import {
  commitWorkspaceFile,
  createTestRepo,
  findSidebarBranchElement,
  openRepo,
} from "../../utils";

describe("ShowWorkspace - stack panel", () => {
  let repoPath: string;
  let user: ReturnType<typeof userEvent.setup>;

  beforeEach(() => {
    ({ repoPath } = createTestRepo(false));
    openRepo(repoPath);
    user = userEvent.setup();
  });

  async function stackBetaOntoAlpha() {
    await createWorkspace(repoPath, "feat/alpha");
    await createWorkspace(repoPath, "feat/beta");
    const workspaces = await getWorkspaces(repoPath);
    const beta = workspaces.find((ws) => ws.branch_name === "feat/beta");
    if (!beta) {
      throw new Error("Expected feat/beta workspace");
    }
    await setWorkspaceTargetBranch(
      repoPath,
      getFullWorkspacePath(beta),
      beta.id,
      "feat/alpha",
    );
  }

  it("does not render the stack panel for the main repository or a lone default-branch workspace", async () => {
    await createWorkspace(repoPath, "feat/alpha");
    render(<Dashboard />);

    await screen.findByText("Go to file");
    expect(
      screen.queryByTestId("workspace-stack-panel"),
    ).not.toBeInTheDocument();

    await user.click(await findSidebarBranchElement("feat/alpha"));
    await screen.findByText("Go to file");
    await waitFor(() => {
      expect(
        screen.queryByTestId("workspace-stack-panel"),
      ).not.toBeInTheDocument();
    });
  });

  it("renders the stack for a workspace stacked on top of another workspace and navigates on click", async () => {
    await stackBetaOntoAlpha();
    render(<Dashboard />);

    await user.click(await findSidebarBranchElement("feat/beta"));

    const panel = await screen.findByTestId("workspace-stack-panel");
    expect(within(panel).getByText("1 of 2")).toBeTruthy();
    expect(within(panel).getByText("feat/alpha")).toBeTruthy();

    await user.click(within(panel).getByText("feat/alpha"));

    const header = await screen.findByTestId("show-workspace-header");
    await within(header).findByText("feat/alpha");

    const rootPanel = await screen.findByTestId("workspace-stack-panel");
    expect(within(rootPanel).getByText("2 of 2")).toBeTruthy();
    expect(within(rootPanel).getByText("feat/beta")).toBeTruthy();
  });

  it("does not list sibling workspaces of the current workspace on the stack card", async () => {
    await createWorkspace(repoPath, "feat/alpha");
    await createWorkspace(repoPath, "feat/beta");
    await createWorkspace(repoPath, "feat/gamma");
    const workspaces = await getWorkspaces(repoPath);
    const beta = workspaces.find((ws) => ws.branch_name === "feat/beta");
    const gamma = workspaces.find((ws) => ws.branch_name === "feat/gamma");
    if (!beta || !gamma) {
      throw new Error("Expected feat/beta and feat/gamma workspaces");
    }
    await setWorkspaceTargetBranch(
      repoPath,
      getFullWorkspacePath(beta),
      beta.id,
      "feat/alpha",
    );
    await setWorkspaceTargetBranch(
      repoPath,
      getFullWorkspacePath(gamma),
      gamma.id,
      "feat/alpha",
    );

    render(<Dashboard />);
    await user.click(await findSidebarBranchElement("feat/beta"));

    const panel = await screen.findByTestId("workspace-stack-panel");
    expect(within(panel).getByText("feat/alpha")).toBeTruthy();
    expect(within(panel).getByText("feat/beta")).toBeTruthy();
    expect(within(panel).queryByText("feat/gamma")).not.toBeInTheDocument();
  });

  it("shows the stack panel when viewing the first workspace of a stack", async () => {
    await stackBetaOntoAlpha();
    render(<Dashboard />);

    await user.click(await findSidebarBranchElement("feat/alpha"));

    const panel = await screen.findByTestId("workspace-stack-panel");
    expect(within(panel).getByText("2 of 2")).toBeTruthy();
    expect(within(panel).getByText("feat/beta")).toBeTruthy();
    expect(within(panel).getByText("feat/alpha")).toBeTruthy();
  });

  it("shows real line-change counts for a stacked workspace with commits", async () => {
    await stackBetaOntoAlpha();

    const beta = (await getWorkspaces(repoPath)).find(
      (ws) => ws.branch_name === "feat/beta",
    )!;
    await commitWorkspaceFile(
      repoPath,
      { id: beta.id, path: beta.workspace_path },
      "feature.txt",
      "line one\nline two\nline three",
      "Add feature file",
    );

    render(<Dashboard />);
    await user.click(await findSidebarBranchElement("feat/beta"));

    const panel = await screen.findByTestId("workspace-stack-panel");
    const betaItem = within(panel)
      .getByText("feat/beta")
      .closest("button") as HTMLElement;

    await waitFor(() => {
      expect(betaItem.textContent).toMatch(/\+\d/);
    });
  });
});
