/**
 * Agent terminal columns: the header controls, the focus highlight after a
 * close and after a workspace switch, the maximized pane's controls, and a
 * model change relaunching the agent without its task.
 */
import userEvent from "@testing-library/user-event";
import { expect, it, onTestFinished } from "vitest";
import { Dashboard } from "../../../src/components/Dashboard";
import { createWorkspace } from "../../../src/lib/api";
import { installFakeAgents } from "../../../test/fake-agent";
import { render, screen, waitFor, within } from "../../../test/test-utils";
import { createTestRepo, openRepo } from "../../../test/utils";
import { captureDocument } from "../capture";
import { clickGpuiRow } from "../../../test/gpui-sidebar";

const agentColumns = () =>
  Array.from(
    document.querySelectorAll<HTMLElement>('[data-terminal-id^="agent-"]'),
  );

const pane = () => screen.getByTestId("workspace-terminal-pane");

it("captures focus moving on close and the maximized pane", async () => {
  onTestFinished(installFakeAgents());
  const { repoPath } = createTestRepo(false);
  openRepo(repoPath);
  const user = userEvent.setup();
  render(<Dashboard />);
  await screen.findByTestId("show-workspace-header");

  for (let count = 1; count <= 3; count++) {
    await user.keyboard("{Meta>}]{/Meta}");
    await waitFor(() => expect(agentColumns()).toHaveLength(count), {
      timeout: 15000,
    });
  }
  await waitFor(
    () => expect(within(pane()).getAllByText("ready")).toHaveLength(3),
    { timeout: 15000 },
  );
  const [, middle, last] = agentColumns();
  await user.click(middle);
  await user.click(
    within(middle).getByRole("button", { name: "Close session" }),
  );
  await waitFor(() => expect(agentColumns()).toHaveLength(2));
  expect(last).toHaveAttribute("data-active", "true");
  await captureDocument(document, {
    name: "terminal-pane-sessions-01-after-close",
    expectations: [
      "Two agent columns remain, 'Claude 1' and 'Claude 3'; the 'Claude 3' header is highlighted blue as the focused column.",
      "Each header shows only the queue, model, search and close controls: no reset or scroll-to-bottom button.",
    ],
  });

  await user.click(
    within(pane()).getByRole("button", { name: "Maximize terminal" }),
  );
  await captureDocument(document, {
    name: "terminal-pane-sessions-02-maximized",
    expectations: [
      "The terminal pane fills the whole main area, covering the workspace header.",
      "The pane's restore and collapse buttons are visible in its bottom-right corner, over the terminal body.",
    ],
  });
}, 120000);

it("captures the highlight after switching workspace", async () => {
  onTestFinished(installFakeAgents());
  const { repoPath } = createTestRepo(false);
  openRepo(repoPath);
  await createWorkspace(repoPath, "feat/alpha");
  await createWorkspace(repoPath, "feat/beta");
  const user = userEvent.setup();
  render(<Dashboard />);
  await screen.findByTestId("show-workspace-header");

  for (const branch of ["feat/alpha", "feat/beta"]) {
    await clickGpuiRow(branch);
    await waitFor(() =>
      expect(screen.getByTestId("show-workspace-header")).toHaveTextContent(
        branch,
      ),
    );
    const input = await screen.findByTestId("agent-task-input");
    await user.type(
      within(input).getByPlaceholderText("Describe a task..."),
      `Work on ${branch}`,
    );
    await user.click(within(input).getByRole("button", { name: /^edit$/i }));
    await within(pane()).findByText(
      `prompt: Work on ${branch}`,
      {},
      { timeout: 15000 },
    );
  }
  await clickGpuiRow("feat/alpha");
  await waitFor(() =>
    expect(agentColumns()[0]).toHaveAttribute("data-active", "true"),
  );
  await captureDocument(document, {
    name: "terminal-pane-sessions-03-switched-workspace",
    expectations: [
      "The page shows feat/alpha, and the 'Work on feat/alpha' column header is highlighted blue.",
      "The 'Work on feat/beta' column header is the plain gray one.",
    ],
  });
}, 120000);

it("captures a model change relaunching the agent without its task", async () => {
  onTestFinished(installFakeAgents());
  const { repoPath } = createTestRepo(false);
  openRepo(repoPath);
  const user = userEvent.setup();
  render(<Dashboard />);
  await screen.findByTestId("show-workspace-header");

  const input = await screen.findByTestId("agent-task-input");
  await user.type(
    within(input).getByPlaceholderText("Describe a task..."),
    "Refactor the parser",
  );
  await user.click(within(input).getByRole("button", { name: /^edit$/i }));
  await within(pane()).findByText("ready", {}, { timeout: 15000 });
  await user.click(
    within(agentColumns()[0]).getByRole("button", { name: /^Model:/ }),
  );
  await user.click(await screen.findByRole("menuitem", { name: /^Opus$/ }));
  await within(pane()).findByText("model: opus", {}, { timeout: 15000 });
  await captureDocument(document, {
    name: "terminal-pane-sessions-04-model-change",
    expectations: [
      "The agent terminal shows a fresh launch: 'prompt: <none>' and 'model: opus'.",
      "The header's model label reads Opus.",
    ],
  });
}, 120000);
