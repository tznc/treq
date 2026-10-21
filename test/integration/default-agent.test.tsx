// @include-parallel
import * as React from "react";
import { beforeEach, describe, expect, it } from "vitest";
import { createTestRepo, openRepo } from "../utils";
import {
  createWorkspace,
  getSetting,
  getRepoSetting,
  getSessions,
  setSetting,
} from "../../src/lib/api";
import { render, screen, waitFor, within } from "../test-utils";
import { Dashboard } from "../../src/components/Dashboard";
import { AGENTS } from "../../src/lib/agents";
import userEvent from "@testing-library/user-event";
import fs from "node:fs";
import path from "node:path";

import { waitFor as waitForSnapshot } from "@testing-library/react";
import {
  clickGpuiRow,
  findGpuiToast,
  gpuiSnapshot,
  sendGpuiAction,
} from "../gpui-sidebar";

async function openGpuiSettings() {
  await waitForSnapshot(() => expect(gpuiSnapshot()).toBeTruthy());
  sendGpuiAction({ kind: "openSettings" });
}

describe("default agent configuration", () => {
  let user: ReturnType<typeof userEvent.setup>;
  let repoPath: string;

  beforeEach(() => {
    ({ repoPath } = createTestRepo(false));
    openRepo(repoPath);
    user = userEvent.setup();
  });

  it("lists every supported agent in every agent picker", async () => {
    const agentValues = (select: HTMLElement) =>
      within(select)
        .getAllByRole("option")
        .map((option) => (option as HTMLOptionElement).value)
        .filter(Boolean);
    const everyAgent = AGENTS.map((agent) => agent.id);

    render(<Dashboard />);
    expect(
      agentValues(await screen.findByRole("combobox", { name: "Agent" })),
    ).toEqual(everyAgent);

    await openGpuiSettings();
    expect(agentValues(await screen.findByLabelText("Default Agent"))).toEqual(
      everyAgent,
    );
    expect(agentValues(await screen.findByLabelText("Review Agent"))).toEqual(
      everyAgent,
    );

    await user.click(await screen.findByRole("tab", { name: /application/i }));
    expect(agentValues(await screen.findByLabelText("Default Agent"))).toEqual(
      everyAgent,
    );
  });

  it("saves app-level default_agent via Application settings tab", async () => {
    render(<Dashboard />);

    await openGpuiSettings();
    await screen.findByText("Settings");

    const applicationTab = await screen.findByRole("tab", {
      name: /application/i,
    });
    await user.click(applicationTab);

    const agentSelect = await screen.findByLabelText(/default agent/i);
    await user.selectOptions(agentSelect, "codex");

    await user.click(
      await screen.findByRole("button", { name: /save settings/i }),
    );
    await findGpuiToast("Settings Saved");

    const saved = await getSetting("default_agent");
    expect(saved).toBe("codex");
  });

  it("saves repo-level default_agent via Repository settings tab and it overrides app-level in TaskInput", async () => {
    await setSetting("default_agent", "codex");

    await createWorkspace(repoPath, "feat/agent-settings-test");

    render(<Dashboard />);

    await openGpuiSettings();
    await screen.findByText("Settings");

    const repositoryTab = await screen.findByRole("tab", {
      name: /repository/i,
    });
    expect(repositoryTab).toHaveAttribute("data-state", "active");

    const agentSelect = await screen.findByLabelText(/default agent/i);
    await user.selectOptions(agentSelect, "claude");

    await user.click(
      await screen.findByRole("button", { name: /save settings/i }),
    );
    await findGpuiToast("Settings saved");

    const savedRepo = await getRepoSetting(repoPath, "default_agent");
    expect(savedRepo).toBe("claude");

    await user.click(await screen.findByRole("button", { name: "Close" }));

    await clickGpuiRow("feat/agent-settings-test");

    const agentPicker = await screen.findByLabelText("Agent");
    await waitFor(() => expect(agentPicker).toHaveValue("claude"));
  });

  it("only adds optional Treq gitignore entries after repository opt-in", async () => {
    render(<Dashboard />);
    await openGpuiSettings();

    const gitignorePath = path.join(repoPath, ".gitignore");
    await waitFor(
      () => expect(fs.readFileSync(gitignorePath, "utf8")).toContain(".treq/"),
      { timeout: 15000 },
    );
    expect(fs.readFileSync(gitignorePath, "utf8")).not.toContain(".jj*/");

    await user.click(
      await screen.findByRole("switch", {
        name: /ignore generated Treq paths/i,
      }),
    );
    await user.click(
      await screen.findByRole("button", { name: /save settings/i }),
    );
    await findGpuiToast("Settings saved");

    const gitignore = fs.readFileSync(gitignorePath, "utf8");
    expect(gitignore).toContain(".jj*/");
    expect(gitignore).toContain(".agents/skills/treq*/");
    expect(gitignore).toContain(".claude/skills/treq*/");
  }, 30000);

  it("creates sessions with the selected agent and switching the dropdown changes the agent used", async () => {
    await setSetting("default_agent", "codex");

    await createWorkspace(repoPath, "feat/agent-switch-test");

    render(<Dashboard />);

    await clickGpuiRow("feat/agent-switch-test");

    const agentPicker = await screen.findByLabelText("Agent");
    await waitFor(() => expect(agentPicker).toHaveValue("codex"));

    const textarea = await screen.findByPlaceholderText(/describe a task/i);
    await user.click(textarea);
    await user.type(textarea, "codex task one");
    await user.keyboard("{Control>}{Enter}{/Control}");

    await waitFor(async () => {
      const sessions = await getSessions(repoPath);
      expect(sessions.some((s) => s.name === "codex task one")).toBe(true);
    });

    await user.selectOptions(agentPicker, "claude");
    expect(agentPicker).toHaveValue("claude");
    expect(
      await screen.findByLabelText(/set as default for this repo/i),
    ).toBeInTheDocument();

    const textarea2 = await screen.findByPlaceholderText(/describe a task/i);
    await user.click(textarea2);
    await user.type(textarea2, "claude task two");
    await user.keyboard("{Control>}{Enter}{/Control}");

    await waitFor(async () => {
      const sessions = await getSessions(repoPath);
      expect(sessions.some((s) => s.name === "claude task two")).toBe(true);
    });
    await waitFor(() =>
      expect(
        screen.queryByLabelText(/set as default for this repo/i),
      ).not.toBeInTheDocument(),
    );

    await user.selectOptions(agentPicker, "codex");
    expect(agentPicker).toHaveValue("codex");
  });

  it("saves app-level default_agent as cursor and TaskInput picks it up", async () => {
    await setSetting("default_agent", "cursor");

    await createWorkspace(repoPath, "feat/agent-cursor-test");

    render(<Dashboard />);

    await clickGpuiRow("feat/agent-cursor-test");

    const agentPicker = await screen.findByLabelText("Agent");
    await waitFor(() => expect(agentPicker).toHaveValue("cursor"));
  });

  it("command palette agent prompt uses the configured default_agent", async () => {
    await setSetting("default_agent", "codex");

    await createWorkspace(repoPath, "feat/agent-terminal-default-test");

    render(<Dashboard />);

    await clickGpuiRow("feat/agent-terminal-default-test");

    await user.keyboard("{Meta>}k{/Meta}");
    await user.click(
      await screen.findByText("Start a new agent session with a prompt"),
    );

    const promptDialog = (
      await screen.findByRole("heading", {
        name: "Start a new agent session",
      })
    ).closest('[data-testid="modal"]');
    if (!promptDialog) throw new Error("Agent prompt dialog was not rendered");
    const textarea =
      await within(promptDialog).findByPlaceholderText(/describe a task/i);
    await user.type(textarea, "command palette codex task");
    await user.click(within(promptDialog).getByRole("button", { name: "Run" }));

    await waitFor(async () => {
      const sessions = await getSessions(repoPath);
      expect(
        sessions.some((s) => s.name === "command palette codex task"),
      ).toBe(true);
    });
  });

  it("opens the agent prompt dialog with the home repo selected on Meta+I", async () => {
    render(<Dashboard />);

    await user.keyboard("{Meta>}i{/Meta}");

    const promptDialog = (
      await screen.findByRole("heading", {
        name: "Start a new agent session",
      })
    ).closest('[data-testid="modal"]');
    if (!promptDialog) throw new Error("Agent prompt dialog was not rendered");

    expect(
      within(promptDialog).getByPlaceholderText(/describe a task/i),
    ).toBeTruthy();
  });

  it("new agent terminal shortcut uses the configured default_agent, not hardcoded claude", async () => {
    await setSetting("default_agent", "codex");

    await createWorkspace(repoPath, "feat/agent-pane-button-test");

    render(<Dashboard />);

    await clickGpuiRow("feat/agent-pane-button-test");

    await screen.findByTestId("workspace-terminal-pane");
    await user.keyboard("{Meta>}]{/Meta}");

    await waitFor(async () => {
      const sessions = await getSessions(repoPath);
      expect(sessions.some((s) => s.name === "Codex 1")).toBe(true);
    });
  });
});
