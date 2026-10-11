import { beforeEach, describe, expect, it, vi } from "vitest";
import userEvent from "@testing-library/user-event";
import { render, screen, waitFor } from "../../test/test-utils";
import { TaskInput } from "./TaskInput";

const api = vi.hoisted(() => ({
  createSession: vi.fn(),
  getSetting: vi.fn(),
  getRepoSetting: vi.fn(),
  setRepoSetting: vi.fn(),
  searchWorkspaceFiles: vi.fn(),
  getWorkspaces: vi.fn(),
  githubOpenOrCreateWorkspaceFromIssue: vi.fn(),
}));

const linearApi = vi.hoisted(() => ({
  linearOpenOrCreateWorkspaceFromIssue: vi.fn(),
}));

vi.mock("../lib/api", async (importOriginal) => {
  const original = await importOriginal<typeof import("../lib/api")>();
  return {
    ...original,
    createSession: api.createSession,
    getSetting: api.getSetting,
    getRepoSetting: api.getRepoSetting,
    setRepoSetting: api.setRepoSetting,
    searchWorkspaceFiles: api.searchWorkspaceFiles,
    getWorkspaces: api.getWorkspaces,
    githubOpenOrCreateWorkspaceFromIssue:
      api.githubOpenOrCreateWorkspaceFromIssue,
  };
});

vi.mock("@tauri-apps/plugin-dialog", () => ({
  open: vi.fn(),
}));

vi.mock("../lib/api-linear", async (importOriginal) => ({
  ...(await importOriginal<typeof import("../lib/api-linear")>()),
  ...linearApi,
}));

describe("TaskInput issue chip", () => {
  let user: ReturnType<typeof userEvent.setup>;

  beforeEach(() => {
    user = userEvent.setup();
    api.createSession.mockReset();
    api.createSession.mockResolvedValue(99);
    api.getSetting.mockResolvedValue("claude");
    api.getRepoSetting.mockResolvedValue(null);
    api.setRepoSetting.mockResolvedValue(undefined);
    api.searchWorkspaceFiles.mockResolvedValue([]);
    api.getWorkspaces.mockResolvedValue([]);
    linearApi.linearOpenOrCreateWorkspaceFromIssue.mockReset();
    api.githubOpenOrCreateWorkspaceFromIssue.mockReset();
    api.githubOpenOrCreateWorkspaceFromIssue.mockResolvedValue({
      workspace_id: 5,
      created: true,
    });
  });

  it.each([
    "success",
    "failure",
  ])("shows loading while starting a session and clears it after %s", async (outcome) => {
    let resolve!: (id: number) => void;
    let reject!: (error: Error) => void;
    api.createSession.mockReturnValue(
      new Promise<number>((res, rej) => {
        resolve = res;
        reject = rej;
      }),
    );
    const onSessionCreated = vi.fn();
    render(
      <TaskInput
        repoPath="/repo"
        workspaceId={1}
        workingDirectory="/repo"
        initialText="Fix the bug"
        onSessionCreated={onSessionCreated}
      />,
    );
    await user.selectOptions(
      screen.getByRole("combobox", { name: "Agent" }),
      "codex",
    );
    await user.click(screen.getByRole("button", { name: "Run" }));

    const starting = await screen.findByRole("button", { name: "Starting…" });
    expect(starting).toBeDisabled();
    expect(starting).toHaveAttribute("aria-busy", "true");
    await user.keyboard("{Control>}{Enter}{/Control}");
    expect(api.createSession).toHaveBeenCalledTimes(1);

    if (outcome === "success") resolve(99);
    else reject(new Error("Session unavailable"));

    await waitFor(() => {
      expect(
        screen.queryByRole("button", { name: "Starting…" }),
      ).not.toBeInTheDocument();
    });
    const run = screen.getByRole("button", { name: "Run" });
    expect(run).toHaveAttribute("aria-busy", "false");
    if (outcome === "success") {
      expect(onSessionCreated).toHaveBeenCalledTimes(1);
      expect(run).toBeDisabled();
    } else {
      expect(run).toBeEnabled();
      expect(screen.getByPlaceholderText("Describe a task...")).toHaveValue(
        "Fix the bug",
      );
      expect(
        await screen.findByText("Session unavailable"),
      ).toBeInTheDocument();
    }
  });

  it("renders a dismissible issue chip from initialIssue", async () => {
    render(
      <TaskInput
        repoPath="/repo"
        workspaceId={null}
        workingDirectory="/repo"
        initialIssue={{
          source: "github",
          id: "42",
          key: "#42",
          url: "https://github.com/acme/treq/issues/42",
          title: "Fix the login bug",
          includeSubItems: false,
          subItemIds: [],
        }}
      />,
    );

    const chip = await screen.findByTestId("issue-chip");
    expect(chip).toHaveTextContent("GitHub #42");

    await user.click(
      screen.getByRole("button", { name: /remove github issue/i }),
    );
    expect(screen.queryByTestId("issue-chip")).not.toBeInTheDocument();
  });

  it("starts a GitHub issue's session in the issue's workspace with the issue in the prompt", async () => {
    const onSessionCreated = vi.fn();
    render(
      <TaskInput
        repoPath="/repo"
        workspaceId={1}
        workingDirectory="/repo/.treq/feat"
        onSessionCreated={onSessionCreated}
        initialIssue={{
          source: "github",
          id: "42",
          key: "#42",
          url: "https://github.com/acme/treq/issues/42",
          title: "Fix the login bug",
          includeSubItems: false,
          subItemIds: [],
        }}
      />,
    );

    await screen.findByTestId("issue-chip");
    await user.type(
      screen.getByPlaceholderText("Describe a task..."),
      "fix auth",
    );
    await user.click(screen.getByRole("button", { name: /^edit$/i }));

    await waitFor(() => {
      expect(onSessionCreated).toHaveBeenCalled();
    });
    expect(onSessionCreated.mock.calls[0][0].pendingPrompt).toContain(
      "GitHub issue #42:",
    );
    expect(onSessionCreated.mock.calls[0][0].pendingPrompt).toContain(
      "fix auth",
    );
    expect(api.githubOpenOrCreateWorkspaceFromIssue).toHaveBeenCalledWith(
      "/repo",
      42,
      "Fix the login bug",
      "https://github.com/acme/treq/issues/42",
    );
    expect(onSessionCreated.mock.calls[0][0].workspaceId).toBe(5);
  });

  it("identifies the workspace without sending a working directory", async () => {
    const onSessionCreated = vi.fn();
    render(
      <TaskInput
        repoPath="/repo"
        workspaceId={1}
        workingDirectory="/repo/.treq/workspaces/feature-one"
        onSessionCreated={onSessionCreated}
      />,
    );

    await user.type(
      screen.getByPlaceholderText("Describe a task..."),
      "fix the workspace bug",
    );
    await user.click(screen.getByRole("button", { name: /^edit$/i }));

    await waitFor(() => expect(onSessionCreated).toHaveBeenCalled());
    expect(onSessionCreated).toHaveBeenCalledWith(
      expect.objectContaining({
        workspaceId: 1,
        workspacePath: null,
      }),
    );
  });

  it("creates the Linear issue workspace before creating its agent session", async () => {
    const onSessionCreated = vi.fn();
    linearApi.linearOpenOrCreateWorkspaceFromIssue.mockResolvedValue({
      issue_id: "issue-id",
      workspace_id: 7,
      created: true,
    });
    api.getWorkspaces.mockResolvedValue([
      {
        id: 7,
        repo_path: "/repo",
        workspace_name: "treq-281",
        workspace_path: ".treq/workspaces/treq-281",
        branch_name: "ty/treq-281-linear-integration",
        created_at: "now",
        title: "Linear integration should CRUD issues",
        not_on_remote: true,
      },
    ]);
    render(
      <TaskInput
        repoPath="/repo"
        workspaceId={null}
        workingDirectory="/repo"
        onSessionCreated={onSessionCreated}
        initialIssue={{
          source: "linear",
          id: "issue-id",
          key: "TREQ-281",
          title: "Linear integration should CRUD issues",
          url: "https://linear.app/treq/issue/TREQ-281",
          includeSubItems: false,
          subItemIds: [],
        }}
      />,
    );

    expect(await screen.findByTestId("issue-chip")).toHaveTextContent(
      "Linear TREQ-281",
    );
    await user.click(screen.getByRole("button", { name: /^edit$/i }));

    await waitFor(() => expect(api.createSession).toHaveBeenCalled());
    expect(
      linearApi.linearOpenOrCreateWorkspaceFromIssue,
    ).toHaveBeenCalledTimes(1);
    expect(linearApi.linearOpenOrCreateWorkspaceFromIssue).toHaveBeenCalledWith(
      "/repo",
      "issue-id",
    );
    expect(api.createSession).toHaveBeenCalledWith(
      "/repo",
      7,
      expect.stringContaining("TREQ-281"),
    );
    expect(onSessionCreated).toHaveBeenCalledWith(
      expect.objectContaining({
        workspaceId: 7,
        workspacePath: null,
        pendingPrompt: expect.stringContaining("Linear issue TREQ-281"),
      }),
    );
  });

  it("reports sub-issue workspaces created, opened, and failed", async () => {
    const outcomes: Record<
      string,
      { workspace_id: number; created: boolean } | Error
    > = {
      "issue-id": { workspace_id: 7, created: true },
      "child-a": { workspace_id: 8, created: true },
      "child-b": { workspace_id: 9, created: false },
      "child-c": new Error(
        "Failed to create workspace for Linear issue TREQ-290",
      ),
      "child-d": new Error("Issue child-d not found"),
    };
    linearApi.linearOpenOrCreateWorkspaceFromIssue.mockImplementation(
      async (_repo: string, id: string) => {
        const outcome = outcomes[id];
        if (outcome instanceof Error) throw outcome;
        return { issue_id: id, ...outcome };
      },
    );
    render(
      <TaskInput
        repoPath="/repo"
        workspaceId={null}
        workingDirectory="/repo"
        initialIssue={{
          source: "linear",
          id: "issue-id",
          key: "TREQ-281",
          title: "Linear integration should CRUD issues",
          url: "https://linear.app/treq/issue/TREQ-281",
          includeSubItems: true,
          subItemIds: ["child-a", "child-b", "child-c", "child-d"],
        }}
      />,
    );

    await user.click(screen.getByRole("button", { name: /^edit$/i }));

    await waitFor(() => expect(api.createSession).toHaveBeenCalled());
    expect(api.createSession).toHaveBeenCalledWith(
      "/repo",
      7,
      expect.any(String),
    );
    expect(
      await screen.findByText("Sub-issue workspaces ready"),
    ).toBeInTheDocument();
    expect(
      screen.getByText("Created 1 and opened 1 existing."),
    ).toBeInTheDocument();
    expect(
      screen.getByText("2 sub-issue workspaces failed"),
    ).toBeInTheDocument();
    expect(
      screen.getByText(
        "Failed to create workspace for Linear issue TREQ-290; Issue child-d not found",
      ),
    ).toBeInTheDocument();
    expect(
      linearApi.linearOpenOrCreateWorkspaceFromIssue.mock.calls.map(
        (c) => c[1],
      ),
    ).toEqual(["issue-id", "child-a", "child-b", "child-c", "child-d"]);
  });
});
