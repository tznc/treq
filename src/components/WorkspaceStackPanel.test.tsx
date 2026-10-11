import userEvent from "@testing-library/user-event";
import { openUrl } from "@tauri-apps/plugin-opener";
import { beforeEach, describe, expect, it, vi } from "vitest";
import { createMockWorkspace } from "../../test/factories/workspace.factory";
import { render, screen, waitFor, within } from "../../test/test-utils";
import type {
  JjLogResult,
  Workspace,
  WorkspaceSidebarStatus,
} from "../lib/api";
import * as api from "../lib/api";
import { WEB_URL } from "../lib/supabase";
import {
  type ActiveRepository,
  repositoryCacheKey,
} from "../lib/active-repository";
import { ActiveRepositoryProvider } from "../lib/active-repository-context";
import { invalidateQueries } from "../lib/swr-cache";
import { WorkspaceStackPanel } from "./WorkspaceStackPanel";

vi.mock("../lib/api", async () => {
  const actual =
    await vi.importActual<typeof import("../lib/api")>("../lib/api");
  return {
    ...actual,
    listWorkspaceStatuses: vi.fn(),
    listCommits: vi.fn(),
  };
});

function asStatuses(workspaces: Workspace[]): WorkspaceSidebarStatus[] {
  return workspaces.map((current) => ({ current, has_conflicts: false }));
}

function makeLogResult(insertions: number, deletions: number): JjLogResult {
  return {
    commits:
      insertions === 0 && deletions === 0
        ? []
        : [
            {
              commit_id: "c1",
              short_id: "c1",
              change_id: "chg1",
              description: "Some change",
              author_name: "Test User",
              timestamp: new Date().toISOString(),
              parent_ids: [],
              is_working_copy: false,
              bookmarks: [],
              is_immutable: false,
              insertions,
              deletions,
              on_target_only: false,
            },
          ],
    target_branch: "main",
    workspace_branch: "ws",
  };
}

const rootWorkspace = createMockWorkspace({
  id: 1,
  branch_name: "chore/refactor",
  workspace_name: "chore/refactor",
  title: "chore: refactor review components",
  target_branch: "main",
  created_at: new Date(Date.now() - 2 * 60 * 60 * 1000).toISOString(),
});

const middleWorkspace = createMockWorkspace({
  id: 2,
  branch_name: "feat/context-prompts",
  workspace_name: "feat/context-prompts",
  title: "feat: add context-aware refactoring prompts",
  target_branch: "chore/refactor",
  created_at: new Date(Date.now() - 28 * 60 * 1000).toISOString(),
});

const tipWorkspace = createMockWorkspace({
  id: 3,
  branch_name: "feat/ai-summaries",
  workspace_name: "feat/ai-summaries",
  title: "feat: introduce AI-powered review summaries",
  target_branch: "feat/context-prompts",
  created_at: new Date(Date.now() - 11 * 60 * 1000).toISOString(),
});

describe("WorkspaceStackPanel", () => {
  beforeEach(() => {
    vi.clearAllMocks();
  });

  it("renders nothing when a lone workspace has no stacked descendants", async () => {
    vi.mocked(api.listWorkspaceStatuses).mockResolvedValue(
      asStatuses([rootWorkspace]),
    );
    vi.mocked(api.listCommits).mockResolvedValue(makeLogResult(0, 0));

    render(
      <WorkspaceStackPanel
        repoPath={rootWorkspace.repo_path}
        workspace={rootWorkspace}
        defaultBranch="main"
      />,
    );

    await waitFor(() => {
      expect(api.listWorkspaceStatuses).toHaveBeenCalled();
    });
    expect(
      screen.queryByTestId("workspace-stack-panel"),
    ).not.toBeInTheDocument();
  });

  it("shows the stack when viewing the first (root) workspace of a stack", async () => {
    vi.mocked(api.listWorkspaceStatuses).mockResolvedValue(
      asStatuses([rootWorkspace, middleWorkspace, tipWorkspace]),
    );
    vi.mocked(api.listCommits).mockResolvedValue(makeLogResult(0, 0));

    render(
      <WorkspaceStackPanel
        repoPath={rootWorkspace.repo_path}
        workspace={rootWorkspace}
        defaultBranch="main"
      />,
    );

    await screen.findByText("Stack");
    expect(await screen.findByText("3 of 3")).toBeTruthy();
    const currentItem = await screen.findByTestId(
      `workspace-stack-item-${rootWorkspace.id}`,
    );
    expect(currentItem.getAttribute("aria-current")).toBe("true");
  });

  it("shows the stack header with the current position out of the total", async () => {
    vi.mocked(api.listWorkspaceStatuses).mockResolvedValue(
      asStatuses([rootWorkspace, middleWorkspace, tipWorkspace]),
    );
    vi.mocked(api.listCommits).mockResolvedValue(makeLogResult(0, 0));

    render(
      <WorkspaceStackPanel
        repoPath={middleWorkspace.repo_path}
        workspace={middleWorkspace}
        defaultBranch="main"
      />,
    );

    await screen.findByText("Stack");
    expect(await screen.findByText("2 of 3")).toBeTruthy();
  });

  it("shows a help tooltip explaining what a stack is", async () => {
    vi.mocked(api.listWorkspaceStatuses).mockResolvedValue(
      asStatuses([rootWorkspace, middleWorkspace, tipWorkspace]),
    );
    vi.mocked(api.listCommits).mockResolvedValue(makeLogResult(0, 0));
    const user = userEvent.setup();

    render(
      <WorkspaceStackPanel
        repoPath={middleWorkspace.repo_path}
        workspace={middleWorkspace}
        defaultBranch="main"
      />,
    );

    const helpButton = await screen.findByRole("button", {
      name: /what is a stack/i,
    });
    await user.hover(helpButton);

    const tooltip = await screen.findByRole("tooltip");
    expect(tooltip).toHaveTextContent(
      /chain of workspaces that build on each other/i,
    );
    expect(
      within(tooltip).getByRole("button", { name: /learn more/i }),
    ).toBeTruthy();
  });

  it("opens the stacks docs when the help tooltip learn-more link is clicked", async () => {
    vi.mocked(api.listWorkspaceStatuses).mockResolvedValue(
      asStatuses([rootWorkspace, middleWorkspace, tipWorkspace]),
    );
    vi.mocked(api.listCommits).mockResolvedValue(makeLogResult(0, 0));
    vi.mocked(openUrl).mockClear();
    // Base UI's tooltip hover-safe-area briefly toggles `pointer-events` on
    // ancestor elements based on real pointer trajectory; userEvent's
    // synthetic instant pointer jump can race that transition even though a
    // real mouse path never triggers it, so the pointer-events safety check
    // is disabled for this interaction.
    const user = userEvent.setup({ pointerEventsCheck: 0 });

    render(
      <WorkspaceStackPanel
        repoPath={middleWorkspace.repo_path}
        workspace={middleWorkspace}
        defaultBranch="main"
      />,
    );

    const helpButton = await screen.findByRole("button", {
      name: /what is a stack/i,
    });
    await user.hover(helpButton);

    const tooltip = await screen.findByRole("tooltip");
    await user.click(
      within(tooltip).getByRole("button", { name: /learn more/i }),
    );

    expect(openUrl).toHaveBeenCalledWith(
      `${WEB_URL}/docs/concepts/workspaces#stacks-and-rebasing`,
    );
  });

  it("renders every workspace title in the stack, tip-first", async () => {
    vi.mocked(api.listWorkspaceStatuses).mockResolvedValue(
      asStatuses([rootWorkspace, middleWorkspace, tipWorkspace]),
    );
    vi.mocked(api.listCommits).mockResolvedValue(makeLogResult(0, 0));

    render(
      <WorkspaceStackPanel
        repoPath={middleWorkspace.repo_path}
        workspace={middleWorkspace}
        defaultBranch="main"
      />,
    );

    await screen.findByText(tipWorkspace.title!);
    const titles = screen
      .getAllByTestId(/^workspace-stack-item-/)
      .map((el) => el.textContent);
    expect(titles[0]).toContain(tipWorkspace.title);
    expect(titles[1]).toContain(middleWorkspace.title);
    expect(titles[2]).toContain(rootWorkspace.title);
  });

  it("marks the current workspace's stack item distinctly from the others", async () => {
    vi.mocked(api.listWorkspaceStatuses).mockResolvedValue(
      asStatuses([rootWorkspace, middleWorkspace, tipWorkspace]),
    );
    vi.mocked(api.listCommits).mockResolvedValue(makeLogResult(0, 0));

    render(
      <WorkspaceStackPanel
        repoPath={middleWorkspace.repo_path}
        workspace={middleWorkspace}
        defaultBranch="main"
      />,
    );

    const currentItem = await screen.findByTestId(
      `workspace-stack-item-${middleWorkspace.id}`,
    );
    const otherItem = await screen.findByTestId(
      `workspace-stack-item-${tipWorkspace.id}`,
    );
    expect(currentItem.getAttribute("aria-current")).toBe("true");
    expect(otherItem.getAttribute("aria-current")).toBeNull();
  });

  it("navigates to a different workspace when its stack item is clicked", async () => {
    vi.mocked(api.listWorkspaceStatuses).mockResolvedValue(
      asStatuses([rootWorkspace, middleWorkspace, tipWorkspace]),
    );
    vi.mocked(api.listCommits).mockResolvedValue(makeLogResult(0, 0));
    const onSelectWorkspace = vi.fn();
    const user = userEvent.setup();

    render(
      <WorkspaceStackPanel
        repoPath={middleWorkspace.repo_path}
        workspace={middleWorkspace}
        defaultBranch="main"
        onSelectWorkspace={onSelectWorkspace}
      />,
    );

    const tipItem = await screen.findByTestId(
      `workspace-stack-item-${tipWorkspace.id}`,
    );
    await user.click(tipItem);

    expect(onSelectWorkspace).toHaveBeenCalledWith(tipWorkspace);
  });

  it("shows the number of line changes for each stacked workspace", async () => {
    vi.mocked(api.listWorkspaceStatuses).mockResolvedValue(
      asStatuses([rootWorkspace, middleWorkspace, tipWorkspace]),
    );
    vi.mocked(api.listCommits).mockImplementation(
      async (_repoPath, workspaceId) =>
        workspaceId === tipWorkspace.id
          ? makeLogResult(11, 24)
          : makeLogResult(0, 0),
    );

    render(
      <WorkspaceStackPanel
        repoPath={middleWorkspace.repo_path}
        workspace={middleWorkspace}
        defaultBranch="main"
      />,
    );

    const tipItem = await screen.findByTestId(
      `workspace-stack-item-${tipWorkspace.id}`,
    );
    await waitFor(() => {
      expect(tipItem.textContent).toContain("+11");
      expect(tipItem.textContent).toContain("-24");
    });
  });

  it("does not list sibling workspaces of the current workspace", async () => {
    const siblingWorkspace = createMockWorkspace({
      id: 4,
      branch_name: "feat/unrelated-sibling",
      workspace_name: "feat/unrelated-sibling",
      title: "feat: unrelated sibling",
      target_branch: "chore/refactor",
    });
    const childOfCurrent = createMockWorkspace({
      id: 5,
      branch_name: "feat/child-of-middle",
      workspace_name: "feat/child-of-middle",
      title: "feat: child of middle",
      target_branch: "feat/context-prompts",
    });
    const siblingOfChild = createMockWorkspace({
      id: 6,
      branch_name: "feat/other-child-of-middle",
      workspace_name: "feat/other-child-of-middle",
      title: "feat: other child of middle",
      target_branch: "feat/context-prompts",
    });

    vi.mocked(api.listWorkspaceStatuses).mockResolvedValue(
      asStatuses([
        rootWorkspace,
        middleWorkspace,
        tipWorkspace,
        siblingWorkspace,
        childOfCurrent,
        siblingOfChild,
      ]),
    );
    vi.mocked(api.listCommits).mockResolvedValue(makeLogResult(0, 0));

    render(
      <WorkspaceStackPanel
        repoPath={middleWorkspace.repo_path}
        workspace={middleWorkspace}
        defaultBranch="main"
      />,
    );

    await screen.findByText("Stack");
    expect(screen.queryByText(siblingWorkspace.title!)).not.toBeInTheDocument();
    expect(screen.getByText(childOfCurrent.title!)).toBeTruthy();
    expect(screen.getByText(siblingOfChild.title!)).toBeTruthy();
    expect(screen.getByText(rootWorkspace.title!)).toBeTruthy();
  });

  it("uses shared LOC number-column widths so bar midlines align across the stack", async () => {
    vi.mocked(api.listWorkspaceStatuses).mockResolvedValue(
      asStatuses([rootWorkspace, middleWorkspace, tipWorkspace]),
    );
    vi.mocked(api.listCommits).mockImplementation(
      async (_repoPath, workspaceId) => {
        if (workspaceId === tipWorkspace.id) return makeLogResult(253, 18277);
        if (workspaceId === middleWorkspace.id) return makeLogResult(61, 61);
        return makeLogResult(18724, 477);
      },
    );

    render(
      <WorkspaceStackPanel
        repoPath={middleWorkspace.repo_path}
        workspace={middleWorkspace}
        defaultBranch="main"
      />,
    );

    const indicators = await screen.findAllByTestId("workspace-loc-indicator");
    expect(indicators).toHaveLength(3);
    const columns = indicators.map((el) => el.style.gridTemplateColumns);
    expect(new Set(columns).size).toBe(1);
    expect(columns[0]).toBe("6ch auto 6ch");
  });

  it("refetches in a remote repo when the repository-keyed statuses are invalidated", async () => {
    const endpoint = {
      id: "endpoint-1",
      hostname: "box",
    } as unknown as NonNullable<ActiveRepository["endpoint"]>;
    const remoteRepo: ActiveRepository = {
      id: "remote-1",
      location: { type: "ssh", host: "box", path: rootWorkspace.repo_path },
      endpoint,
      endpointId: "endpoint-1",
      endpointGeneration: 1,
      canonicalPath: rootWorkspace.repo_path,
      displayName: "project",
      transport: { type: "ssh", endpoint },
    };
    vi.mocked(api.listWorkspaceStatuses).mockResolvedValue(
      asStatuses([rootWorkspace, middleWorkspace]),
    );
    vi.mocked(api.listCommits).mockResolvedValue(makeLogResult(0, 0));

    render(
      <ActiveRepositoryProvider repository={remoteRepo}>
        <WorkspaceStackPanel
          repoPath={rootWorkspace.repo_path}
          workspace={rootWorkspace}
          defaultBranch="main"
        />
      </ActiveRepositoryProvider>,
    );
    await waitFor(() => {
      expect(api.listWorkspaceStatuses).toHaveBeenCalledTimes(1);
    });

    // Dashboard invalidates by repository identity after a stack change.
    await invalidateQueries([
      "workspace-statuses",
      repositoryCacheKey(remoteRepo),
    ]);

    await waitFor(() => {
      expect(api.listWorkspaceStatuses).toHaveBeenCalledTimes(2);
    });
  });
});
