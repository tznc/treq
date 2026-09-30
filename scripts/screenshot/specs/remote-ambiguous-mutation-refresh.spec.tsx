import * as React from "react";
import { expect, it } from "vitest";
import userEvent from "@testing-library/user-event";
import { createTestRepo, openRepo } from "../../../test/utils";
import { act, render, screen, waitFor, within } from "../../../test/test-utils";
import { Dashboard } from "../../../src/components/Dashboard";
import { useRemoteMutationFeedback } from "../../../src/lib/remote-mutation-ui";
import { captureDocument } from "../capture";

it("captures the Refresh action on the ambiguous remote mutation dialog", async () => {
  const { repoPath } = createTestRepo(false);
  openRepo(repoPath);
  const user = userEvent.setup();
  render(<Dashboard />);
  await screen.findByTestId("show-workspace-header");

  act(() => {
    useRemoteMutationFeedback.getState().report({
      status: "ambiguous",
      reason: "Could not tell whether the remote commit landed.",
    });
  });
  const dialog = await screen.findByRole("dialog", {
    name: "Remote change could not be verified",
  });
  await captureDocument(document, {
    name: "remote-ambiguous-mutation-refresh-01-dialog",
    expectations: [
      "A dialog titled 'Remote change could not be verified' is shown over the app.",
      "The dialog tells the user to refresh before trying again and shows the reason text.",
      "An outlined 'Dismiss' button sits left of a filled primary 'Refresh' button.",
    ],
  });

  await user.click(within(dialog).getByRole("button", { name: "Refresh" }));
  await waitFor(() => expect(screen.queryByRole("dialog")).toBeNull());
  await captureDocument(document, {
    name: "remote-ambiguous-mutation-refresh-02-closed",
    expectations: [
      "The dialog is gone and the workspace view is visible with no overlay.",
    ],
  });
}, 60000);
