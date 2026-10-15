import { beforeEach, describe, expect, it, vi } from "vitest";
import {
  createWorkspace,
  getRepoSetting,
  getWorkspaces,
  listRepoBranches,
} from "./api";
import { createTaskWorkspace, taskBranchName } from "./taskWorkspace";

vi.mock("./api", () => ({
  createWorkspace: vi.fn(),
  getRepoSetting: vi.fn(),
  getWorkspaces: vi.fn(),
  listRepoBranches: vi.fn(),
}));

describe("createTaskWorkspace", () => {
  beforeEach(() => {
    vi.clearAllMocks();
    vi.mocked(getWorkspaces).mockResolvedValue([]);
    vi.mocked(listRepoBranches).mockResolvedValue([]);
    vi.mocked(createWorkspace).mockResolvedValue(42);
  });

  it.each([
    [null, "treq/fix-bug"],
    ["", "treq/fix-bug"],
    ["team/{name}", "team/fix-bug"],
    ["{name}", "fix-bug"],
  ])("uses the repo pattern %s", async (pattern, expected) => {
    vi.mocked(getRepoSetting).mockResolvedValue(pattern);

    expect(await createTaskWorkspace("/repo", "Fix bug", "Fix bug")).toBe(42);

    expect(getRepoSetting).toHaveBeenCalledWith("/repo", "branch_name_pattern");
    expect(createWorkspace).toHaveBeenCalledWith(
      "/repo",
      expected,
      undefined,
      JSON.stringify({ title: "Fix bug" }),
    );
  });
});

describe("taskBranchName", () => {
  it("checks collisions after applying the branch pattern", () => {
    expect(
      taskBranchName(
        "Fix bug",
        new Set(["team/fix-bug", "team/fix-bug-2"]),
        "team/{name}",
      ),
    ).toBe("team/fix-bug-3");
  });

  it("names the branch after the prompt's words", () => {
    expect(taskBranchName("Fix: the login redirect (iOS)!", new Set())).toBe(
      "fix-the-login-redirect-ios",
    );
  });

  it("cuts a long prompt on a word boundary", () => {
    expect(
      taskBranchName(
        "Rework the ranking pipeline so it handles multi currency invoices correctly",
        new Set(),
      ),
    ).toBe("rework-the-ranking-pipeline-so-it-handles-multi");
  });

  it("falls back to task when the prompt has no ASCII letters or digits", () => {
    expect(taskBranchName("✨ 修复", new Set())).toBe("task");
  });

  it("adds a number when a workspace or branch already uses the name", () => {
    expect(taskBranchName("Fix bug", new Set(["fix-bug", "fix-bug-2"]))).toBe(
      "fix-bug-3",
    );
  });
});
