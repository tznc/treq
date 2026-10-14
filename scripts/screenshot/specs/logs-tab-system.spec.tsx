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
      'The Logs tab is active and the "Log source" select reads "System".',
      "Log lines show time, level, a Source column (treq_lib::sync, webview, treq_lib::pty) and the message; the WARN line is amber and the ERROR line red.",
      'An "Open log folder" button sits next to the level filter.',
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
