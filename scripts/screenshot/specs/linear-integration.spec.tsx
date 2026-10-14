import * as React from "react";
import { it } from "vitest";
import userEvent from "@testing-library/user-event";
import { createTestRepo, openRepo } from "../../../test/utils";
import { render, screen } from "../../../test/test-utils";
import { Dashboard } from "../../../src/components/Dashboard";
import { captureDocument } from "../capture";
import { sendGpuiAction } from "../../../test/gpui-sidebar";

it("captures the Linear panel", async () => {
  const { repoPath } = createTestRepo(false);
  openRepo(repoPath);

  render(<Dashboard />);

  sendGpuiAction({ kind: "openLinear" });
  await screen.findByTestId("linear-panel");

  await captureDocument(document, {
    name: "linear-integration-02-panel-no-api-key",
    expectations: [
      "The Linear panel is open with List/Kanban view tabs visible.",
      "Since no API key or OAuth connection is configured for this repo, an error or empty state is shown instead of an issue list.",
    ],
  });

  // The team selector dropdown is captured in
  // linear-views-filtering-projects.spec.tsx: inside the full Dashboard, jsdom
  // gives Base UI a zero-size anchor, so the popup lands under the sidebar.
}, 60000);

it("captures the Linear integration settings", async () => {
  const { repoPath } = createTestRepo(false);
  openRepo(repoPath);

  const user = userEvent.setup();
  render(<Dashboard />);

  sendGpuiAction({ kind: "openSettings" });
  await user.click(await screen.findByRole("tab", { name: /integrations/i }));
  await screen.findByRole("tab", { name: /integrations/i, selected: true });
  await screen.findByRole("heading", { name: "Linear" });

  await captureDocument(document, {
    name: "linear-integration-03-settings",
    expectations: [
      "The Settings page Integrations tab is open with a Linear section.",
      "An API key input control and an 'Add API Key' (or similar) button are visible for the free-tier flow.",
    ],
  });
}, 60000);

// The ShowWorkspace "linked to Linear issue" badge (data-testid
// "linear-issue-badge") and the backend write that populates it are
// covered by core::workspaces::merge_linear_issue_metadata_persists_to_real_workspace
// in src-tauri -- there's no test-only way to seed workspace metadata from
// this harness without a production-only command whose sole caller would be
// this spec, so that plumbing is verified at the Rust layer instead.
