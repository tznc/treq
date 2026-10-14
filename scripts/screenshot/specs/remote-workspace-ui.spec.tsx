import * as React from "react";
import userEvent from "@testing-library/user-event";
import { it } from "vitest";
import { Dashboard } from "../../../src/components/Dashboard";
import { createWorkspace } from "../../../src/lib/api";
import { useRemoteCutoffStore } from "../../../src/stores/remoteCutoffStore";
import { render, screen } from "../../../test/test-utils";
import { openSavedRemoteRepo } from "../../../test/remote-repo";
import { createTestRepo } from "../../../test/utils";
import { captureDocument } from "../capture";
import { findGpuiRow } from "../../../test/gpui-sidebar";

it("captures the remote workspace tree and state banners", async () => {
  const { repoPath } = createTestRepo(false);
  await createWorkspace(repoPath, "feat/remote-qa");
  await openSavedRemoteRepo(repoPath, { endpointId: "ep-qa", generation: 1 });
  window.history.replaceState({}, "", "/");

  const user = userEvent.setup();
  render(<Dashboard />);
  await screen.findByTestId("show-workspace-header");
  await findGpuiRow("feat/remote-qa");
  await user.click(await screen.findByRole("button", { name: "Refresh" }));

  await captureDocument(document, {
    name: "remote-workspace-connected",
    expectations: [
      "The normal workspace header and sidebar are shown for a remote repository.",
      "A Refresh control and a capability notice about native SSH PTY are visible.",
      "There is no 'Remote repository connected' placeholder or Remote review panel.",
    ],
  });

  useRemoteCutoffStore.getState().recordCutoff("ep-qa", "key_revoked");
  await screen.findByTestId("remote-status-banner");
  await captureDocument(document, {
    name: "remote-workspace-cutoff",
    expectations: [
      "A credential cutoff banner tells the user to reauthenticate.",
      "The workspace tree remains laid out behind the banner.",
    ],
  });
});
