// @include-parallel
import * as React from "react";
import { beforeEach, describe, expect, it } from "vitest";
import { createTestRepo, findSidebarBranchElement, openRepo } from "../utils";
import { act, render, screen, waitFor } from "../test-utils";
import { Dashboard } from "../../src/components/Dashboard";
import userEvent from "@testing-library/user-event";
import {
  createWorkspace,
<<<<<<< HEAD
  scheduleWorkspaces,
=======
  runWorkflow,
>>>>>>> e591c89 (Remove workspace scheduling feature)
  setSetting,
} from "../../src/lib/api";
import {
  trackerListItems,
  trackerStartAutoKickoffPolling,
} from "../../src/lib/api-tracker";
import { previewSettingKey } from "../../src/lib/features";
import { useFeaturePreviewStore } from "../../src/stores/featurePreviewStore";

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
    expect(
      screen.getByRole("button", { name: "Trello integration" }),
    ).toBeTruthy();
    expect(
      screen.getByRole("button", { name: "Jira integration" }),
    ).toBeTruthy();
    expect(screen.queryByRole("button", { name: /merge queue/i })).toBeNull();
    expect(screen.queryByRole("switch", { name: /merge queue/i })).toBeNull();
  });

  it("hides Trello and Jira from the sidebar and Integrations when disabled", async () => {
    await openFeaturePreview();
    await user.click(await screen.findByRole("tab", { name: /integrations/i }));
    expect(
      await screen.findByTestId("trello-integration-settings"),
    ).toBeTruthy();
    expect(screen.getByTestId("jira-integration-settings")).toBeTruthy();

    await user.click(
      await screen.findByRole("tab", { name: /feature preview/i }),
    );
    await user.click(
      screen.getByRole("switch", { name: "Trello integration" }),
    );
    await user.click(screen.getByRole("switch", { name: "Jira integration" }));
    await user.click(await screen.findByRole("tab", { name: /integrations/i }));

    expect(screen.queryByTestId("trello-integration-settings")).toBeNull();
    expect(screen.queryByTestId("jira-integration-settings")).toBeNull();
    expect(screen.queryByTestId("trello-sidebar-item")).toBeNull();
    expect(screen.queryByTestId("jira-sidebar-item")).toBeNull();
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

describe("tracker preview routes", () => {
  let user: ReturnType<typeof userEvent.setup>;

  beforeEach(async () => {
    const { repoPath } = createTestRepo(false);
    openRepo(repoPath);
    user = userEvent.setup();
    await setSetting(previewSettingKey("trelloIntegration"), "true");
    useFeaturePreviewStore.getState().hydrateFlags({
      [previewSettingKey("trelloIntegration")]: "true",
    });
  });

  it("leaves the Trello panel when Trello integration is turned off", async () => {
    render(<Dashboard />);

    await user.click(await screen.findByTestId("trello-sidebar-item"));
    expect(await screen.findByTestId("trello-panel")).toBeTruthy();

    act(() => {
      void useFeaturePreviewStore
        .getState()
        .setPreviewFeature("trelloIntegration", false);
    });

    await waitFor(() => {
      expect(screen.queryByTestId("trello-panel")).toBeNull();
      expect(window.location.pathname.startsWith("/trello")).toBe(false);
    });
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

describe("feature preview rust gates", () => {
  it("rejects Trello and Jira commands when their preview flags are off", async () => {
    const { repoPath } = createTestRepo(false);
    await setSetting(previewSettingKey("trelloIntegration"), "false");
    await setSetting(previewSettingKey("jiraIntegration"), "false");

    await expect(trackerListItems("trello", repoPath)).rejects.toThrow(
      /trelloIntegration/,
    );
    await expect(
      trackerStartAutoKickoffPolling("jira", repoPath),
    ).rejects.toThrow(/jiraIntegration/);
  });
});
