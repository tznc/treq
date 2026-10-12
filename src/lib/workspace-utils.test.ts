import { describe, expect, it } from "vitest";
import type { Workspace } from "./api-types";
import { getWorkspaceDisplayTitle } from "./workspace-utils";

function makeWorkspace(overrides: Partial<Workspace> = {}): Workspace {
  return {
    id: 1,
    repo_path: "/tmp/repo",
    workspace_name: "feature/a",
    workspace_path: "ws/feature-a",
    branch_name: "feature/a",
    created_at: "2026-01-01T00:00:00.000Z",
    title: "",
    not_on_remote: false,
    ...overrides,
  };
}

describe("getWorkspaceDisplayTitle", () => {
  it("returns the workspace title when set", () => {
    const workspace = makeWorkspace({ title: "Add usage guidelines" });
    expect(getWorkspaceDisplayTitle(workspace)).toBe("Add usage guidelines");
  });

  it("falls back to metadata.title when title is empty", () => {
    const workspace = makeWorkspace({
      title: "",
      metadata: JSON.stringify({ title: "From metadata" }),
    });
    expect(getWorkspaceDisplayTitle(workspace)).toBe("From metadata");
  });

  it("falls back to branch_name when neither title nor metadata title exist", () => {
    const workspace = makeWorkspace({ title: "", branch_name: "docs/ai" });
    expect(getWorkspaceDisplayTitle(workspace)).toBe("docs/ai");
  });

  it("falls back to branch_name when metadata is malformed JSON", () => {
    const workspace = makeWorkspace({
      title: "",
      metadata: "{not valid json",
      branch_name: "docs/ai",
    });
    expect(getWorkspaceDisplayTitle(workspace)).toBe("docs/ai");
  });
});
