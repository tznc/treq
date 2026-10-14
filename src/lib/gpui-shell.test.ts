import { describe, expect, it } from "vitest";
import { createMockWorkspace } from "../../test/factories/workspace.factory";
import type { PrInfo } from "./api-types";
import { buildGpuiSidebarSnapshot } from "./gpui-shell";
import {
  buildWorkspaceTree,
  flattenWorkspaceTree,
  type WorkspaceSidebarStatusWithAgentDetail,
} from "./workspace-tree";

let nextId = 0;

function status(
  branch: string,
  extra: Partial<WorkspaceSidebarStatusWithAgentDetail> = {},
  target: string | null = null,
): WorkspaceSidebarStatusWithAgentDetail {
  return {
    current: createMockWorkspace({
      id: ++nextId,
      branch_name: branch,
      target_branch: target,
    }),
    has_conflicts: false,
    ...extra,
  };
}

function pr(state: string, isDraft?: boolean): PrInfo {
  return {
    number: 1,
    title: "t",
    state,
    url: "u",
    head_ref_name: "h",
    base_ref_name: "b",
    merge_state_status: null,
    is_draft: isDraft,
  };
}

function build(
  statuses: WorkspaceSidebarStatusWithAgentDetail[],
  overrides: Partial<Parameters<typeof buildGpuiSidebarSnapshot>[0]> = {},
) {
  return buildGpuiSidebarSnapshot({
    repoPath: "/home/me/code/treq",
    nodes: flattenWorkspaceTree(buildWorkspaceTree(statuses)),
    streamingByBranchWithAgentSession: new Map(),
    prStatusesByBranch: {},
    hasRemote: true,
    selectedWorkspaceId: null,
    currentPage: "session",
    homeBranch: "main",
    linearEnabled: false,
    dark: true,
    width: 280,
    loading: false,
    ...overrides,
  });
}

describe("buildGpuiSidebarSnapshot", () => {
  it("orders rows like the React tree and carries stack depth", () => {
    const snapshot = build([
      status("child", {}, "root"),
      status("root", {}, null),
      status("grandchild", {}, "child"),
    ]);
    expect(snapshot.workspaces.map((w) => [w.title, w.depth])).toEqual([
      ["root", 0],
      ["child", 1],
      ["grandchild", 2],
    ]);
    expect(snapshot).toMatchObject({
      repoName: "treq",
      homeBranch: "main",
      dark: true,
      width: 280,
      loading: false,
    });
  });

  it("marks the GitHub row active on its page and hides Linear when off", () => {
    const snapshot = build([status("a", {}, null)], { currentPage: "github" });
    expect(snapshot.github).toBe(true);
    expect(snapshot.linear).toBeNull();
    expect(snapshot.homeSelected).toBe(false);
  });

  it("shows the Linear row when its preview is on", () => {
    const onPage = build([], { linearEnabled: true, currentPage: "linear" });
    expect(onPage.linear).toBe(true);
    expect(onPage.github).toBe(false);
    expect(build([], { linearEnabled: true }).linear).toBe(false);
  });

  it("selects home when no workspace is selected", () => {
    const snapshot = build([status("a", {}, null)]);
    expect(snapshot.homeSelected).toBe(true);
    expect(snapshot.settingsOpen).toBe(false);
    expect(snapshot.workspaces[0].selected).toBe(false);
  });

  it("selects the chosen workspace and not home", () => {
    const b = status("b", {}, null);
    const snapshot = build([status("a", {}, null), b], {
      selectedWorkspaceId: b.current.id,
    });
    expect(snapshot.homeSelected).toBe(false);
    expect(snapshot.workspaces.map((w) => w.selected).sort()).toEqual([
      false,
      true,
    ]);
    expect(
      snapshot.workspaces.find((w) => w.id === b.current.id)?.selected,
    ).toBe(true);
  });

  it.each([
    "settings",
    "github",
    "linear",
  ])("drops home and workspace highlight on the %s page", (currentPage) => {
    const a = status("a", {}, null);
    const snapshot = build([a], {
      currentPage,
      selectedWorkspaceId: a.current.id,
    });
    expect(snapshot.homeSelected).toBe(false);
    expect(snapshot.workspaces[0].selected).toBe(false);
    expect(snapshot.settingsOpen).toBe(currentPage === "settings");
  });

  it("maps conflicts, changes and clean workspaces to status dots", () => {
    const snapshot = build([
      status("conflict", { has_conflicts: true }, null),
      status("dirty", { has_changes: true }, null),
      status("clean", {}, null),
    ]);
    const byTitle = Object.fromEntries(
      snapshot.workspaces.map((w) => [w.title, w.status]),
    );
    expect(byTitle.conflict).toEqual({ color: "red", spin: false });
    expect(byTitle.dirty).toEqual({ color: "yellow", spin: false });
    expect(byTitle.clean).toBeNull();
  });

  it("spins only when the branch has an agent session that is streaming", () => {
    const snapshot = build(
      [
        status("streaming", { has_changes: true }, null),
        status("idle", { has_changes: true }, null),
        status("clean-streaming", {}, null),
        status("no-session", {}, null),
      ],
      {
        streamingByBranchWithAgentSession: new Map([
          ["streaming", true],
          ["idle", false],
          ["clean-streaming", true],
        ]),
      },
    );
    const byTitle = Object.fromEntries(
      snapshot.workspaces.map((w) => [w.title, w.status]),
    );
    expect(byTitle.streaming).toEqual({ color: "yellow", spin: true });
    expect(byTitle.idle).toEqual({ color: "yellow", spin: false });
    expect(byTitle["clean-streaming"]).toEqual({
      color: "neutral",
      spin: true,
    });
    expect(byTitle["no-session"]).toBeNull();
  });

  it("maps PR state, treating a draft only while open", () => {
    const snapshot = build(
      [
        status("draft", {}, null),
        status("open", {}, null),
        status("merged", {}, null),
        status("closed", {}, null),
        status("none", {}, null),
        status("draft-merged", {}, null),
      ],
      {
        prStatusesByBranch: {
          draft: pr("OPEN", true),
          open: pr("OPEN"),
          merged: pr("MERGED"),
          closed: pr("CLOSED"),
          "draft-merged": pr("MERGED", true),
        },
      },
    );
    const byTitle = Object.fromEntries(
      snapshot.workspaces.map((w) => [w.title, w.pr]),
    );
    expect(byTitle).toEqual({
      draft: "draft",
      open: "open",
      merged: "merged",
      closed: "closed",
      none: null,
      "draft-merged": "merged",
    });
  });

  it("omits PR state without a GitHub remote", () => {
    const snapshot = build([status("open", {}, null)], {
      hasRemote: false,
      prStatusesByBranch: { open: pr("OPEN") },
    });
    expect(snapshot.workspaces[0].pr).toBeNull();
  });

  it("falls back to a generic repository name", () => {
    expect(build([], { repoPath: undefined }).repoName).toBe("Repository");
  });
});
