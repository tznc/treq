// @include-parallel
import userEvent from "@testing-library/user-event";
import { afterEach, beforeEach, describe, expect, it } from "vitest";
import { Dashboard } from "../../src/components/Dashboard";
import { createWorkspace } from "../../src/lib/api";
import { installFakeAgents } from "../fake-agent";
import { render, screen, waitFor, within } from "../test-utils";
import { createTestRepo, openRepo } from "../utils";

const agentColumns = () =>
  Array.from(
    document.querySelectorAll<HTMLElement>('[data-terminal-id^="agent-"]'),
  );

const pane = () => screen.getByTestId("workspace-terminal-pane");

describe("terminal pane sessions", () => {
  let user: ReturnType<typeof userEvent.setup>;
  let restoreAgents: () => void;
  let repoPath: string;

  beforeEach(() => {
    restoreAgents = installFakeAgents();
    ({ repoPath } = createTestRepo(false));
    openRepo(repoPath);
    user = userEvent.setup();
  });

  afterEach(() => {
    restoreAgents();
  });

  async function submitTask(text: string) {
    const input = await screen.findByTestId("agent-task-input");
    await user.type(
      within(input).getByPlaceholderText("Describe a task..."),
      text,
    );
    await user.click(within(input).getByRole("button", { name: /^edit$/i }));
    await within(pane()).findByText(`prompt: ${text}`, {}, { timeout: 15000 });
  }

  async function startDefaultAgent(expectedColumns: number) {
    await user.keyboard("{Meta>}]{/Meta}");
    await waitFor(() => expect(agentColumns()).toHaveLength(expectedColumns), {
      timeout: 15000,
    });
  }

  async function openWorkspace(branch: string) {
    await user.click(
      await within(screen.getByTestId("workspace-sidebar")).findByText(branch),
    );
    await waitFor(() =>
      expect(screen.getByTestId("show-workspace-header")).toHaveTextContent(
        branch,
      ),
    );
  }

  it("moves focus to the neighbouring column when the active one is closed, so Cmd+W closes that next", async () => {
    render(<Dashboard />);
    await screen.findByTestId("show-workspace-header");
    await startDefaultAgent(1);
    await startDefaultAgent(2);
    await startDefaultAgent(3);
    const [first, middle, last] = agentColumns();

    await user.click(middle);
    expect(middle).toHaveAttribute("data-active", "true");
    await user.click(
      within(middle).getByRole("button", { name: "Close session" }),
    );

    await waitFor(() => expect(agentColumns()).toEqual([first, last]));
    expect(last).toHaveAttribute("data-active", "true");

    await user.keyboard("{Meta>}w{/Meta}");
    await waitFor(() => expect(agentColumns()).toEqual([first]));
  }, 60000);

  it("marks the selected workspace's terminal active after switching workspace", async () => {
    await createWorkspace(repoPath, "feat/alpha");
    await createWorkspace(repoPath, "feat/beta");
    render(<Dashboard />);
    await screen.findByTestId("show-workspace-header");

    await openWorkspace("feat/alpha");
    await submitTask("Work on feat/alpha");
    await openWorkspace("feat/beta");
    await submitTask("Work on feat/beta");

    await openWorkspace("feat/alpha");

    const [alpha, beta] = agentColumns();
    await waitFor(() => expect(alpha).toHaveAttribute("data-active", "true"));
    expect(beta).toHaveAttribute("data-active", "false");
  }, 60000);

  it("has no Reset or Scroll to bottom buttons in the agent header", async () => {
    render(<Dashboard />);
    await screen.findByTestId("show-workspace-header");
    await startDefaultAgent(1);
    const [column] = agentColumns();
    expect(
      within(column).getByRole("button", { name: "Close session" }),
    ).toBeInTheDocument();
    expect(
      within(column).queryByRole("button", { name: "Reset terminal" }),
    ).not.toBeInTheDocument();
    expect(
      within(column).queryByRole("button", { name: "Scroll to bottom" }),
    ).not.toBeInTheDocument();
  }, 60000);
});
