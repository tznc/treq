import * as React from "react";
import { expect, it } from "vitest";
import userEvent from "@testing-library/user-event";
import {
  createTestRepo,
  openRepo,
} from "../../../test/utils";
import { render, screen } from "../../../test/test-utils";
import { Dashboard } from "../../../src/components/Dashboard";
import { captureDocument } from "../capture";
import { sendGpuiAction } from "../../../test/gpui-sidebar";

it("captures the Feature Preview settings tab and gated UI", async () => {
  const { repoPath } = createTestRepo(false);
  openRepo(repoPath);
  const user = userEvent.setup();
  render(<Dashboard />);

  sendGpuiAction({ kind: "openSettings" });
  await user.click(
    await screen.findByRole("tab", { name: /feature preview/i }),
  );
  await screen.findByTestId("feature-preview-settings");

  await captureDocument(document, {
    name: "feature-preview-01-settings-tab",
    expectations: [
      "Settings shows a Feature Preview tab with Skills installation and Remote SSH.",
      "Each feature title is a documentation link and has an on/off switch.",
    ],
  });

  expect(screen.getByRole("tab", { name: /skills/i })).toBeVisible();
  await user.click(screen.getByRole("switch", { name: "Skills installation" }));
  expect(screen.queryByRole("tab", { name: /^skills$/i })).toBeNull();

  await captureDocument(document, {
    name: "feature-preview-02-skills-tab-hidden",
    expectations: [
      "The Skills settings tab is gone after Skills installation is switched off.",
      "Feature Preview remains the selected settings tab.",
    ],
  });
}, 60000);
