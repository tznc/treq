/**
 * Devcontainer environments: pick "Devcontainer" in the task composer, watch
 * Treq start the repository's container, then run an agent and a shell in it.
 * Uses a real Docker engine and the Dev Containers CLI; skipped without them.
 */
import { execSync } from "node:child_process";
import { mkdirSync, writeFileSync } from "node:fs";
import path from "node:path";
import userEvent from "@testing-library/user-event";
import { expect, it, onTestFinished } from "vitest";
import { Dashboard } from "../../../src/components/Dashboard";
import { installFakeAgents } from "../../../test/fake-agent";
import { render, screen, waitFor, within } from "../../../test/test-utils";
import { createTestRepo, openRepo } from "../../../test/utils";
import { captureDocument } from "../capture";

const PROMPT = "Run the test suite inside the container";
const FAKE_AGENT_DIR = path.resolve(__dirname, "../../../test/fake-agent");

const hasDevcontainerTooling = (() => {
  try {
    execSync("docker info && devcontainer --version", { stdio: "ignore" });
    return true;
  } catch {
    return false;
  }
})();

const pane = () => screen.getByTestId("workspace-terminal-pane");

const removeContainer = (repoPath: string) => {
  execSync(
    `docker ps -aq --filter label=treq.repo=${repoPath} | xargs -r docker rm -f`,
    { stdio: "ignore" },
  );
};

const openNewShell = async (user: ReturnType<typeof userEvent.setup>) => {
  await user.hover(screen.getByTestId("home-repo-row"));
  await user.click(
    await within(screen.getByTestId("home-repo-row")).findByRole("button", {
      name: "Open shell",
    }),
  );
};

it.skipIf(!hasDevcontainerTooling)(
  "captures switching agents and terminals into the devcontainer",
  async () => {
    onTestFinished(installFakeAgents());
    const { repoPath } = createTestRepo(false);
    // The fake agent's symlinks point into the checkout; mount it so they
    // resolve inside the container too.
    mkdirSync(path.join(repoPath, ".devcontainer"));
    writeFileSync(
      path.join(repoPath, ".devcontainer/devcontainer.json"),
      JSON.stringify({
        image: "alpine:3.20",
        mounts: [`type=bind,source=${FAKE_AGENT_DIR},target=${FAKE_AGENT_DIR}`],
      }),
    );
    onTestFinished(() => removeContainer(repoPath));
    openRepo(repoPath);

    const user = userEvent.setup();
    render(<Dashboard />);
    await screen.findByTestId("show-workspace-header");

    const composer = await screen.findByTestId("agent-task-input");
    const picker = await within(composer).findByRole("button", {
      name: "Environment: Local",
    });
    await user.click(picker);
    const devcontainerItem = await screen.findByRole("menuitem", {
      name: /Devcontainer\s*Stopped/,
    });
    await captureDocument(document, {
      name: "devcontainer-environment-01-picker-open",
      expectations: [
        "Under the task composer, an environment menu headed 'New terminals and agents run in' lists 'Local' (checked, 'This machine') and 'Devcontainer' ('Stopped').",
        "The composer's environment button reads 'Local' with a laptop icon, next to the agent picker and the Edit button.",
      ],
    });

    await user.click(devcontainerItem);
    await within(composer).findByRole("button", {
      name: /Environment: Devcontainer/,
    });
    await within(composer).findByLabelText("Starting");
    await captureDocument(document, {
      name: "devcontainer-environment-02-starting",
      expectations: [
        "The composer's environment button now reads 'Devcontainer' with a box icon and a spinning loader while the container starts.",
      ],
    });

    await within(composer).findByLabelText("Running", {}, { timeout: 240000 });
    await user.type(
      within(composer).getByPlaceholderText("Describe a task..."),
      PROMPT,
    );
    await user.click(within(composer).getByRole("button", { name: /^edit$/i }));
    await within(pane()).findByText(
      `prompt: ${PROMPT}`,
      {},
      { timeout: 30000 },
    );

    await openNewShell(user);
    await waitFor(
      () =>
        expect(
          pane().querySelector('[data-terminal-id^="shell-"]'),
        ).not.toBeNull(),
      { timeout: 15000 },
    );
    const containerShell = pane().querySelector<HTMLElement>(
      '[data-terminal-id^="shell-"]',
    )!;
    await user.click(containerShell.querySelector<HTMLElement>(".xterm")!);
    await user.keyboard("head -1 /etc/os-release; whoami{Enter}");
    await within(pane()).findByText(
      /NAME="Alpine Linux"/,
      {},
      { timeout: 30000 },
    );
    await captureDocument(document, {
      name: "devcontainer-environment-03-running-in-container",
      expectations: [
        `The agent terminal shows the fake agent's output starting with 'prompt: ${PROMPT}', launched inside the container.`,
        "The shell terminal printed 'NAME=\"Alpine Linux\"' and 'root': the container's OS and user, not the host's.",
        "The composer's environment button reads 'Devcontainer' with a green dot, and the terminal pane controls show the same picker.",
      ],
    });

    await user.click(
      within(pane()).getByRole("button", { name: /Environment: Devcontainer/ }),
    );
    await user.click(await screen.findByRole("menuitem", { name: /^Local/ }));
    await within(pane()).findByRole("button", { name: "Environment: Local" });
    await openNewShell(user);
    await waitFor(
      () =>
        expect(
          pane().querySelectorAll('[data-terminal-id^="shell-"]'),
        ).toHaveLength(2),
      { timeout: 15000 },
    );
    const localShell = pane().querySelectorAll<HTMLElement>(
      '[data-terminal-id^="shell-"]',
    )[1];
    await user.click(localShell.querySelector<HTMLElement>(".xterm")!);
    await user.keyboard("head -1 /etc/os-release{Enter}");
    await waitFor(
      () => {
        expect(localShell.textContent).toMatch(/NAME="(?!Alpine)/);
      },
      { timeout: 30000 },
    );
    await captureDocument(document, {
      name: "devcontainer-environment-04-back-to-local",
      expectations: [
        "After switching the pane's picker back to 'Local', a second shell prints the host's OS name, not 'Alpine Linux'.",
        "The earlier agent and container shell keep running in the container next to the new local shell.",
      ],
    });
  },
  400000,
);
