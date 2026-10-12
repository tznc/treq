// @include-parallel
import { beforeEach, describe, expect, it } from "vitest";
import { render } from "../test-utils";
import { createTestRepo, findSidebarBranchElement, openRepo } from "../utils";
import { createWorkspace } from "../../src/lib/api";
import { Dashboard } from "../../src/components/Dashboard";
import { within } from "@testing-library/react";

describe("workspace sidebar row actions", () => {
  beforeEach(async () => {
    const { repoPath } = createTestRepo(false);
    openRepo(repoPath);
    await createWorkspace(repoPath, "feat/alpha");
  });

  it("offers agent, shell and stack actions on a workspace row", async () => {
    render(<Dashboard />);

    const alphaLabel = await findSidebarBranchElement("feat/alpha");
    const alphaRow = alphaLabel.closest("div") as HTMLElement;

    expect(
      within(alphaRow).getByRole("button", { name: "Start agent" }),
    ).toBeTruthy();
    expect(
      within(alphaRow).getByRole("button", { name: "Open shell" }),
    ).toBeTruthy();
    expect(
      within(alphaRow).getByRole("button", { name: "Stack a workspace" }),
    ).toBeTruthy();
  });
});
