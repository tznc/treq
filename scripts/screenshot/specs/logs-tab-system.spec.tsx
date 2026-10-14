import * as React from "react";
import { it } from "vitest";
import userEvent from "@testing-library/user-event";
import {
  appendSystemLogLine,
  createTestRepo,
  openRepo,
} from "../../../test/utils";
import { act, render, screen, waitFor } from "../../../test/test-utils";
import { emitOnTestBus } from "../../../test/test-event-bus";
import { Dashboard } from "../../../src/components/Dashboard";
import { captureDocument } from "../capture";

it("captures Help > View Logs opening the System log source", async () => {
  const { repoPath } = createTestRepo(false);
  openRepo(repoPath);
  appendSystemLogLine("INFO", "treq_lib::sync", "Synced 3 workspaces");
  appendSystemLogLine("WARN", "webview", "GitHub rate limit at 90%");
  appendSystemLogLine(
    "ERROR",
    "treq_lib::pty",
    "PTY spawn failed: no such file",
  );

  const user = userEvent.setup();
  render(<Dashboard />);
  await screen.findByRole("tab", { name: /^Logs/, selected: false });

  await captureDocument(document, {
    name: "logs-tab-system-01-before-menu",
    expectations: ["The home repo is open on a tab other than Logs."],
  });

  act(() => emitOnTestBus("menu-view-logs"));
  await screen.findByRole("tab", { name: /^Logs/, selected: true });
  await screen.findByText("PTY spawn failed: no such file");

  await captureDocument(document, {
    name: "logs-tab-system-02-system-source",
    expectations: [
      'The Logs tab is active, "Log source" reads "System" and "Open log folder" sits by the level filter.',
      "Log lines render as a table whose Timestamp, Source, Level and Message headers line up with their values.",
      "Only the level cell is coloured (WARN amber, ERROR red); message text stays the default colour.",
    ],
  });

  await user.click(screen.getByRole("textbox", { name: "Search system logs" }));
  await user.paste("pty");
  await waitFor(() => {
    if (screen.queryByText("Synced 3 workspaces"))
      throw new Error("not filtered");
  });
  await captureDocument(document, {
    name: "logs-tab-system-03-searched",
    expectations: [
      'Only the "PTY spawn failed" line remains after searching "pty".',
    ],
  });
}, 60000);
