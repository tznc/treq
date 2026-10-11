import * as React from "react";
import { expect, it } from "vitest";
import userEvent from "@testing-library/user-event";
import {
  createTestRepo,
  findSidebarBranchElement,
  openRepo,
} from "../../../test/utils";
import { render, screen } from "../../../test/test-utils";
import { Dashboard } from "../../../src/components/Dashboard";
import { createWorkspace } from "../../../src/lib/api";
import { captureDocument } from "../capture";

it("shows the Logs tab on the home repo but not on a workspace", async () => {
  const { repoPath } = createTestRepo(false);
  openRepo(repoPath);
  await createWorkspace(repoPath, "feat/no-logs");
  const user = userEvent.setup();
  render(<Dashboard />);

  expect(await screen.findByRole("tab", { name: /^Logs/ })).toBeTruthy();
  await captureDocument(document, {
    name: "logs-tab-home-only-01-home-repo",
    expectations: [
      "The home repo header tab strip includes a Logs tab with a database icon.",
    ],
  });

  await user.click(await findSidebarBranchElement("feat/no-logs"));
  await screen.findByTestId("show-workspace-header");
  expect(screen.queryByRole("tab", { name: /^Logs/ })).toBeNull();
  await captureDocument(document, {
    name: "logs-tab-home-only-02-workspace",
    expectations: [
      "The feat/no-logs workspace is open in the main pane.",
      "Its tab strip has no Logs tab.",
    ],
  });
}, 60000);
