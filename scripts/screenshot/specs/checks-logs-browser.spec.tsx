import * as React from "react";
import { it } from "vitest";
import userEvent from "@testing-library/user-event";
import {
  createTestRepo,
  openRepo,
  seedSetupScriptLogs,
} from "../../../test/utils";
import { render, screen } from "../../../test/test-utils";
import { Dashboard } from "../../../src/components/Dashboard";
import { captureDocument } from "../capture";
import { clickGpuiRow } from "../../../test/gpui-sidebar";

// Emits info, warning and error lines so the level colouring is visible.
const SETUP_SCRIPT =
  "echo 'Compiling treq v0.1.3'; echo 'warning: unused variable x'; echo 'running 2 tests'; echo 'error: assertion failed' 1>&2; exit 1";

it("captures check inspection in the Logs tab", async () => {
  const { repoPath } = createTestRepo(false);
  openRepo(repoPath);
  await seedSetupScriptLogs(repoPath, "feat/logs", SETUP_SCRIPT, 4);

  const user = userEvent.setup();
  render(<Dashboard />);

  await clickGpuiRow("feat/logs");
  await user.click(await screen.findByRole("tab", { name: /^Logs/ }));
  await screen.findByRole("tab", { name: /^Logs/, selected: true });
  await screen.findByTestId("logs-tab");
  await screen.findByText("Compiling treq v0.1.3");

  await captureDocument(document, {
    name: "checks-logs-01-run-history",
    expectations: [
      'The workspace tab row contains a selected "Logs" tab instead of a standalone "Checks" tab.',
      'The Logs tab shows check output including "Compiling treq v0.1.3".',
      "The log browser provides level filtering and a search field for inspecting check output.",
    ],
  });
}, 90000);
