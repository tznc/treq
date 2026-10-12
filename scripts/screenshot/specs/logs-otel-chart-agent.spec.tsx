import * as React from "react";
import { it } from "vitest";
import userEvent from "@testing-library/user-event";
import {
  createTestRepo,
  openRepo,
  seedSetupScriptLogs,
} from "../../../test/utils";
import { render, screen, waitFor, within } from "../../../test/test-utils";
import { Dashboard } from "../../../src/components/Dashboard";
import { captureDocument } from "../capture";

const SETUP_SCRIPT =
  "echo 'Compiling treq v0.1.3'; echo 'warning: unused variable x'; echo 'running 2 tests'; echo 'error: assertion failed' 1>&2";

it("captures the OTel logs tab, chart, selection and templates menu", async () => {
  const { repoPath } = createTestRepo(false);
  openRepo(repoPath);
  // Seed log rows with a workspace setup script run.
  await seedSetupScriptLogs(repoPath, "feat/logs", SETUP_SCRIPT, 4);

  const user = userEvent.setup();
  render(<Dashboard />);

  await user.click(await screen.findByRole("tab", { name: /^Logs/ }));
  await screen.findByText("Compiling treq v0.1.3");
  await screen.findByTestId("logs-timeseries-chart");

  await captureDocument(document, {
    name: "logs-otel-01-browse-with-chart",
    expectations: [
      'The header reads "Checks logs" with the subtitle "OpenTelemetry records · .treq/telemetry-*.db".',
      'The view toggle offers "Browse" and "Logs Explorer" (not "SQL Explorer"), with Browse selected.',
      "A chart area is rendered between the filter row and the log lines, with a legend showing Info, Warn and Error.",
      "The chart draws stacked bars with a numeric y-axis and time labels along the x-axis.",
      "The bars stack in severity order with Info blue at the bottom, Warn amber above it and Error red on top.",
      "The legend sits in its own band above the plot area and does not overlap any bar.",
      "The x-axis has a tick for every time bucket in the range, including quiet ones with no bars.",
      "Time runs oldest-on-the-left to newest-on-the-right along the x-axis.",
      "Ignore the chart's narrow overall width: jsdom reports a zero-width container, so ECharts falls back to a 100px canvas here instead of filling the panel.",
      'Below the chart a toolbar shows "Multi-select", "Select all" and a disabled "Send to agent" button.',
      'A header row below the toolbar labels "Timestamp", "Run", "Job", "Level" and "Message" columns; log lines beneath align a timestamp, a "#" run id, a blue job id and the message under those tightly-fit columns with the message column taking up most of the row width.',
    ],
  });

  // Drag across the first two lines to select a range.
  const lines = await screen.findAllByTestId("repo-log-line");
  await user.pointer([
    { target: lines[0], keys: "[MouseLeft>]" },
    { target: lines[1] },
    { keys: "[/MouseLeft]" },
  ]);
  await waitFor(() => {
    if (!document.querySelector('[data-testid="selection-count"]')) {
      throw new Error("no selection yet");
    }
  });

  await captureDocument(document, {
    name: "logs-otel-02-drag-selection",
    expectations: [
      "The first two log lines are highlighted with a tinted background, marking them selected.",
      'The toolbar reads "2 selected" and offers a "Clear" button.',
      'The "Send to agent" button is now enabled rather than greyed out.',
      "Log lines after the first two are not highlighted.",
    ],
  });

  // Switch to the explorer and open the templates dropdown.
  await user.click(
    await screen.findByRole("button", { name: /Logs Explorer/i }),
  );
  await user.click(await screen.findByTestId("template-menu"));
  await screen.findByText("Newest records across every run");

  await captureDocument(document, {
    name: "logs-otel-03-templates-menu",
    expectations: [
      "An open dropdown lists exactly five query templates.",
      'Each template shows a bold label above a smaller grey description, e.g. "Errors by job" above "Which jobs produce the most ERROR records".',
      'The five labels are "Recent lines", "Errors by job", "Severity by run", "Slowest steps" and "Trace overview".',
      "Ignore where the dropdown sits on the page: jsdom reports zero-sized boxes so Radix cannot anchor it to the trigger in this harness.",
    ],
  });

  await user.keyboard("{Escape}");
  const explorer = await screen.findByTestId("logs-sql-explorer");
  await user.click(
    within(explorer).getByRole("button", { name: /Run query/i }),
  );
  await screen.findByTestId("sql-results");

  await captureDocument(document, {
    name: "logs-otel-04-explorer-results",
    expectations: [
      "The SQL editor shows a query selecting timestamp, severityText, a job_id extracted from attributes, and a message extracted from body, from the logs view.",
      'A result grid below has columns "timestamp", "severityText", "job_id" and "message".',
      'The severityText column contains OpenTelemetry severity names such as "INFO", "WARN" or "ERROR" — not lowercase "info"/"error".',
      'A "Send results to agent" button appears next to "Run query" now that a result set exists.',
    ],
  });
}, 90000);
