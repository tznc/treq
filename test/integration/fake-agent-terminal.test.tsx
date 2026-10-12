// @include-parallel
import userEvent from "@testing-library/user-event";
import { afterEach, beforeEach, describe, expect, it } from "vitest";
import { Dashboard } from "../../src/components/Dashboard";
import { installFakeAgents } from "../fake-agent";
import { render, screen, within } from "../test-utils";
import { createTestRepo, openRepo } from "../utils";

describe("fake agent terminal", () => {
  let user: ReturnType<typeof userEvent.setup>;
  let restoreAgents: () => void;

  beforeEach(() => {
    restoreAgents = installFakeAgents();
    const { repoPath } = createTestRepo(false);
    openRepo(repoPath);
    user = userEvent.setup();
  });

  afterEach(() => {
    restoreAgents();
  });

  it("shows the agent's launch output in the new session's terminal", async () => {
    render(<Dashboard />);
    await screen.findByTestId("show-workspace-header");
    await user.keyboard("{Meta>}i{/Meta}");
    const dialog = (
      await screen.findByRole("heading", { name: "Start a new agent session" })
    ).closest('[data-testid="modal"]') as HTMLElement;
    await user.type(
      within(dialog).getByPlaceholderText("Describe a task..."),
      "Add a changelog entry",
    );
    await user.click(within(dialog).getByRole("button", { name: /^edit$/i }));

    const pane = await screen.findByTestId("workspace-terminal-pane");
    expect(
      await within(pane).findByText(
        "fake-agent: claude",
        {},
        { timeout: 15000 },
      ),
    ).toBeInTheDocument();
    expect(within(pane).getByText("mode: acceptEdits")).toBeInTheDocument();
    expect(
      within(pane).getByText("prompt: Add a changelog entry"),
    ).toBeInTheDocument();
    expect(
      await within(pane).findByText("ready", {}, { timeout: 15000 }),
    ).toBeInTheDocument();
    await new Promise((resolve) => setTimeout(resolve, 500));
    expect(within(pane).queryByText(/^received:/)).not.toBeInTheDocument();
  }, 30000);
});
