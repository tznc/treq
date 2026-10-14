// @include-parallel
import { clickGpuiRow } from "../../gpui-sidebar";
import userEvent from "@testing-library/user-event";
import { beforeAll, beforeEach, describe, expect, it } from "vitest";
import { Dashboard } from "../../../src/components/Dashboard";
import {
  createWorkspace,
  recordAgentChatScreen,
  recordAgentChatUserMessage,
  registerAgentChat,
} from "../../../src/lib/api";
import { render, screen, waitFor, within } from "../../test-utils";
import {
  createTestRepo,
  openRepo,
  seedSetupScriptLogs,
  writeRepoFile,
} from "../../utils";

const SETUP_SCRIPT = "echo hello-from-logs; echo 'error: broke' 1>&2";

let user: ReturnType<typeof userEvent.setup>;

async function openLogsTab() {
  render(<Dashboard />);
  const logsTab = await screen.findByRole("tab", { name: /^Logs/ });
  await user.click(logsTab);
  await screen.findByRole("tab", { name: /^Logs/, selected: true });
}

async function seedLogs(repoPath: string, branch: string) {
  await seedSetupScriptLogs(repoPath, branch, SETUP_SCRIPT, 2);
}

describe("Home repo Logs tab", () => {
  beforeEach(() => {
    user = userEvent.setup();
  });

  it("shows an empty state when no checks have run", async () => {
    const { repoPath } = createTestRepo(false);
    openRepo(repoPath);

    await openLogsTab();

    await screen.findByText(/No check logs recorded yet/i);
  });

  it("offers the check filter without a workspace selected", async () => {
    const { repoPath } = createTestRepo(false);
    await writeRepoFile(
      repoPath,
      ".treq/workflows/ci.yaml",
      "name: Lint\non:\n  workflow_dispatch: {}\njobs:\n  lint:\n    name: Lint\n    steps:\n      - name: Lint\n        run: echo ok\n",
    );
    openRepo(repoPath);

    await openLogsTab();

    const filter = await screen.findByRole("button", {
      name: "Filter by check",
    });
    expect(filter.textContent).toContain("All checks");
    await user.click(filter);
    expect(
      await screen.findByRole("menuitemcheckbox", { name: "Lint" }),
    ).toBeTruthy();
    await user.keyboard("{Escape}");
    expect(screen.queryByTestId("logs-checks-actions")).toBeNull();
  });

  it("is offered inside a workspace too", async () => {
    const { repoPath } = createTestRepo(false);
    openRepo(repoPath);
    await createWorkspace(repoPath, "logs-scope");

    render(<Dashboard />);
    await clickGpuiRow("logs-scope");

    expect(await screen.findByRole("tab", { name: /^Logs/ })).toBeTruthy();
  });

  describe("with a seeded check run", () => {
    let seededRepoPath: string;

    beforeAll(async () => {
      const { repoPath } = createTestRepo(false);
      await seedLogs(repoPath, "logs-seeded");
      seededRepoPath = repoPath;
    });

    it("browses log lines across runs with run and job ids", async () => {
      openRepo(seededRepoPath);

      await openLogsTab();

      await screen.findByText("hello-from-logs");
      const lines = document.querySelectorAll('[data-testid="repo-log-line"]');
      expect(lines.length).toBe(2);
    });

    it("filters the repo-wide logs by level", async () => {
      openRepo(seededRepoPath);

      await openLogsTab();
      await screen.findByText("hello-from-logs");

      await user.click(await screen.findByTestId("log-level-filter"));
      await user.click(
        await screen.findByRole("menuitemcheckbox", { name: /error/i }),
      );

      await waitFor(() => {
        const lines = document.querySelectorAll(
          '[data-testid="repo-log-line"]',
        );
        expect(lines.length).toBe(1);
      });
    });

    it("runs a SQL query in the explorer and renders a result grid", async () => {
      openRepo(seededRepoPath);

      await openLogsTab();
      await user.click(
        await screen.findByRole("button", { name: /Logs Explorer/i }),
      );

      const explorer = await screen.findByTestId("logs-sql-explorer");
      await user.click(
        within(explorer).getByRole("button", { name: /Run query/i }),
      );

      const table = await screen.findByTestId("sql-results");
      expect(within(table).getAllByRole("row").length).toBeGreaterThan(1);
    });

    it("aggregates across runs with a GROUP BY query", async () => {
      openRepo(seededRepoPath);

      await openLogsTab();
      await user.click(
        await screen.findByRole("button", { name: /Logs Explorer/i }),
      );
      await user.click(await screen.findByTestId("template-menu"));
      await user.click(
        await screen.findByRole("menuitem", { name: /Errors by job/i }),
      );

      const explorer = await screen.findByTestId("logs-sql-explorer");
      await user.click(
        within(explorer).getByRole("button", { name: /Run query/i }),
      );

      const table = await screen.findByTestId("sql-results");
      await within(table).findByText("setup");
    });

    it("rejects a non-read-only query with an error", async () => {
      openRepo(seededRepoPath);

      await openLogsTab();
      await user.click(
        await screen.findByRole("button", { name: /Logs Explorer/i }),
      );

      const editor = await screen.findByTestId("sql-editor");
      await user.clear(editor);
      await user.type(editor, "DROP TABLE logs");

      const explorer = await screen.findByTestId("logs-sql-explorer");
      await user.click(
        within(explorer).getByRole("button", { name: /Run query/i }),
      );

      const error = await screen.findByTestId("sql-error");
      expect(error.textContent).toMatch(/read-only/i);
    });

    it("renders a timeseries chart above the feed", async () => {
      openRepo(seededRepoPath);

      await openLogsTab();
      await screen.findByText("hello-from-logs");

      await screen.findByTestId("logs-timeseries-chart");
    });

    it("selects a range of lines by dragging and enables send to agent", async () => {
      openRepo(seededRepoPath);

      await openLogsTab();
      await screen.findByText("hello-from-logs");

      const lines = await screen.findAllByTestId("repo-log-line");
      expect(screen.queryByTestId("send-to-agent")).toBeNull();

      await user.pointer([
        { target: lines[0], keys: "[MouseLeft>]" },
        { target: lines[1] },
        { keys: "[/MouseLeft]" },
      ]);

      await waitFor(() => {
        expect(screen.getByTestId("selection-count").textContent).toMatch(/2 /);
      });
      expect(screen.queryByTestId("send-to-agent")).toBeNull();
    });

    it("toggles individual lines in multi-select mode", async () => {
      openRepo(seededRepoPath);

      await openLogsTab();
      await screen.findByText("hello-from-logs");

      await user.click(await screen.findByTestId("multi-select-toggle"));
      const lines = await screen.findAllByTestId("repo-log-line");
      await user.click(lines[1]);

      await waitFor(() => {
        expect(screen.getByTestId("selection-count").textContent).toMatch(/1 /);
      });

      await user.click(lines[1]);
      await waitFor(() => {
        expect(screen.queryByTestId("selection-count")).toBeNull();
      });
    });

    it("offers sending the whole result set from the explorer", async () => {
      openRepo(seededRepoPath);

      await openLogsTab();
      await user.click(
        await screen.findByRole("button", { name: /Logs Explorer/i }),
      );

      expect(screen.queryByTestId("send-resultset-to-agent")).toBeNull();

      const explorer = await screen.findByTestId("logs-sql-explorer");
      await user.click(
        within(explorer).getByRole("button", { name: /Run query/i }),
      );
      await screen.findByTestId("sql-results");

      await screen.findByTestId("send-resultset-to-agent");
    });

    it("exposes OpenTelemetry columns in the logs view", async () => {
      openRepo(seededRepoPath);

      await openLogsTab();
      await user.click(
        await screen.findByRole("button", { name: /Logs Explorer/i }),
      );

      const editor = await screen.findByTestId("sql-editor");
      await user.clear(editor);
      await user.type(
        editor,
        "SELECT severityText, severityNumber, json_extract(body, '$.message') AS message, traceId, spanId, json_extract(attributes, '$.run_id') AS run_id FROM logs",
      );

      const explorer = await screen.findByTestId("logs-sql-explorer");
      await user.click(
        within(explorer).getByRole("button", { name: /Run query/i }),
      );

      const table = await screen.findByTestId("sql-results");
      await waitFor(() => {
        expect(within(table).getAllByText("ERROR").length).toBeGreaterThan(0);
      });
      await waitFor(() => {
        expect(within(table).getAllByText("1").length).toBeGreaterThan(0);
      });
    });

    it("lists the query templates with descriptions", async () => {
      openRepo(seededRepoPath);

      await openLogsTab();
      await user.click(
        await screen.findByRole("button", { name: /Logs Explorer/i }),
      );
      await user.click(await screen.findByTestId("template-menu"));

      const items = await screen.findAllByRole("menuitem");
      const labels = items.map((item) => item.textContent ?? "");
      expect(labels.some((label) => label.includes("Recent lines"))).toBe(true);
      expect(labels.some((label) => label.includes("Errors by job"))).toBe(
        true,
      );
      await screen.findByText("Newest records across every run");
    });
  });

  it("lists agent chats as a log source group and hides shell terminals", async () => {
    const { repoPath } = createTestRepo(false);
    openRepo(repoPath);

    await registerAgentChat(
      repoPath,
      11,
      "pty-agent",
      "Claude",
      "claude",
      null,
    );
    await recordAgentChatScreen(repoPath, 11, "Welcome to Claude Code");
    await recordAgentChatUserMessage(
      repoPath,
      11,
      "Welcome to Claude Code",
      "please review the diff",
    );
    await recordAgentChatScreen(
      repoPath,
      11,
      "Welcome to Claude Code\nplease review the diff\nLooking at the stacked changes now",
    );

    await openLogsTab();
    await user.selectOptions(
      await screen.findByRole("combobox", { name: /Log source/i }),
      "agent-chats",
    );
    await screen.findByText("please review the diff");
    await screen.findByText(/Looking at the stacked changes now/i);

    const roles = [
      ...document.querySelectorAll('[data-testid="agent-chat-line"]'),
    ].map((line) => line.textContent ?? "");
    expect(roles.some((text) => text.includes("user"))).toBe(true);
    expect(roles.some((text) => text.includes("agent"))).toBe(true);

    const picker = await screen.findByRole("combobox", {
      name: /Agent terminal/i,
    });
    expect(picker).toHaveTextContent(/Claude/);
    expect(screen.queryByText(/Shell/i)).toBeNull();
  });

  it("shows newly registered chats and live partial output without remounting", async () => {
    const { repoPath } = createTestRepo(false);
    openRepo(repoPath);

    await openLogsTab();
    await user.selectOptions(
      await screen.findByRole("combobox", { name: /Log source/i }),
      "agent-chats",
    );
    await screen.findByText(/No agent chat logs yet/i);

    await registerAgentChat(
      repoPath,
      12,
      "pty-live",
      "Live Codex",
      "codex",
      null,
      "investigate the issue",
    );

    const picker = await screen.findByRole("combobox", {
      name: /Agent terminal/i,
    });
    await waitFor(() => expect(picker).toHaveTextContent("Live Codex"));
    await screen.findByText("investigate the issue");

    await recordAgentChatScreen(repoPath, 12, "partial response is streaming");
    await screen.findByText("partial response is streaming");
  });
});
