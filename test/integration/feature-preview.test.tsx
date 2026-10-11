// @include-parallel
import * as React from "react";
import { beforeEach, describe, expect, it } from "vitest";
import { createTestRepo, findSidebarBranchElement, openRepo } from "../utils";
import { render, screen } from "../test-utils";
import { Dashboard } from "../../src/components/Dashboard";
import userEvent from "@testing-library/user-event";
import { createWorkspace } from "../../src/lib/api";
import { previewSettingKey } from "../../src/lib/features";

describe("feature preview settings", () => {
  let user: ReturnType<typeof userEvent.setup>;
  let repoPath: string;

  beforeEach(() => {
    ({ repoPath } = createTestRepo(false));
    openRepo(repoPath);
    user = userEvent.setup();
  });

  async function openFeaturePreview() {
    render(<Dashboard />);
    await user.click(await screen.findByLabelText("Settings"));
    await user.click(
      await screen.findByRole("tab", { name: /feature preview/i }),
    );
    expect(await screen.findByTestId("feature-preview-settings")).toBeTruthy();
  }

  it("lists preview features as documentation links", async () => {
    await openFeaturePreview();
    expect(
      screen.getByRole("button", { name: "Skills installation" }),
    ).toBeTruthy();
    expect(screen.getByRole("button", { name: "Remote SSH" })).toBeTruthy();
    expect(screen.getByRole("button", { name: "Logs" })).toBeTruthy();
    expect(screen.getByRole("button", { name: "Browser" })).toBeTruthy();
    expect(screen.queryByRole("button", { name: /trello/i })).toBeNull();
    expect(screen.queryByRole("button", { name: /jira/i })).toBeNull();
    expect(screen.queryByRole("button", { name: /merge queue/i })).toBeNull();
    expect(screen.queryByRole("switch", { name: /merge queue/i })).toBeNull();
  });

  it("hides Logs and the Diff/Browser switcher when disabled", async () => {
    await createWorkspace(repoPath, "feat/hidden-previews");
    await openFeaturePreview();
    await user.click(screen.getByRole("switch", { name: "Logs" }));
    await user.click(screen.getByRole("switch", { name: "Browser" }));
    await user.click(screen.getByRole("button", { name: "Close" }));

    expect(screen.queryByRole("tab", { name: /^Logs/ })).toBeNull();
    expect(screen.queryByRole("tab", { name: /^Checks/ })).toBeNull();

    await user.click(await findSidebarBranchElement("feat/hidden-previews"));
    await user.click(await screen.findByRole("tab", { name: "Changes" }));
    expect(
      screen.queryByRole("button", { name: "Switch review view" }),
    ).toBeNull();
  });

  it("hides the Skills tab when skills installation is off", async () => {
    await openFeaturePreview();
    expect(screen.getByRole("tab", { name: /skills/i })).toBeVisible();
    await user.click(
      screen.getByRole("switch", { name: "Skills installation" }),
    );
    expect(screen.queryByRole("tab", { name: /^skills$/i })).toBeNull();
  });
});

describe("feature preview onboarding", () => {
  it("hides Open via SSH when remote SSH is off", async () => {
    const { setSetting } = await import("../../src/lib/api");
    await setSetting(previewSettingKey("remoteSsh"), "false");
    const { useFeaturePreviewStore } = await import(
      "../../src/stores/featurePreviewStore"
    );
    useFeaturePreviewStore.getState().hydrateFlags({
      [previewSettingKey("remoteSsh")]: "false",
    });
    window.history.replaceState({}, "", "/");
    render(<Dashboard />);
    expect(
      await screen.findByRole("button", { name: "Open Repository" }),
    ).toBeTruthy();
    expect(
      screen.queryByRole("button", { name: "Open via SSH" }),
    ).not.toBeInTheDocument();
  });
});
