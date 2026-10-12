import userEvent from "@testing-library/user-event";
import { beforeEach, describe, expect, it, vi } from "vitest";
import { IssueDetailPanel } from "./IssueDetail";
import { CreatePrForm } from "./CreatePrForm";
import { PrDetailPanel } from "./PrDetail";
import { render, screen, waitFor } from "../../../test/test-utils";

const api = vi.hoisted(() => ({
  ghViewIssue: vi.fn(),
  ghCreateIssueComment: vi.fn(),
  ghCloseIssue: vi.fn(),
  ghViewPr: vi.fn(),
  ghCreatePrComment: vi.fn(),
  ghSetPrDraft: vi.fn(),
  ghClosePr: vi.fn(),
  ghReopenPr: vi.fn(),
  getPrInfoViaGh: vi.fn(),
  getPrChecksViaGh: vi.fn(),
  ghCreatePr: vi.fn(),
  getWorkspaces: vi.fn(),
  openOrCreateWorkspaceFromPr: vi.fn(),
}));

vi.mock("../../lib/api", async (importOriginal) => {
  const original = await importOriginal<typeof import("../../lib/api")>();
  return {
    ...original,
    ghViewIssue: api.ghViewIssue,
    ghCreateIssueComment: api.ghCreateIssueComment,
    ghCloseIssue: api.ghCloseIssue,
    ghViewPr: api.ghViewPr,
    ghCreatePrComment: api.ghCreatePrComment,
    ghSetPrDraft: api.ghSetPrDraft,
    ghClosePr: api.ghClosePr,
    ghReopenPr: api.ghReopenPr,
    getPrInfoViaGh: api.getPrInfoViaGh,
    getPrChecksViaGh: api.getPrChecksViaGh,
    ghCreatePr: api.ghCreatePr,
    getWorkspaces: api.getWorkspaces,
    openOrCreateWorkspaceFromPr: api.openOrCreateWorkspaceFromPr,
  };
});

function makeDetailPr(overrides: {
  is_draft?: boolean;
  state?: string;
  title?: string;
}) {
  return {
    number: 42,
    title: overrides.title ?? "Feature PR",
    state: overrides.state ?? "OPEN",
    url: "https://github.com/acme/treq/pull/42",
    body: "Body",
    author: { login: "alice" },
    labels: [],
    head_ref_name: "feat",
    base_ref_name: "main",
    merge_state_status: "CLEAN",
    created_at: "2026-01-01T00:00:00Z",
    updated_at: "2026-01-01T00:00:00Z",
    comments: null,
    is_draft: overrides.is_draft ?? false,
  };
}

describe("IssueDetailPanel markdown", () => {
  let user: ReturnType<typeof userEvent.setup>;

  beforeEach(() => {
    user = userEvent.setup();
    api.ghViewIssue.mockResolvedValue({
      number: 42,
      title: "Fix markdown",
      state: "OPEN",
      url: "https://github.com/acme/treq/issues/42",
      body: "Hello **world** and a `code` span",
      author: { login: "alice" },
      created_at: "2026-01-01T00:00:00Z",
      labels: [],
      comments: [
        {
          id: "1",
          author: { login: "bob" },
          body: "Looks *good*",
          created_at: "2026-01-02T00:00:00Z",
        },
      ],
    });
  });

  it("renders issue body and comments with markdown", async () => {
    render(
      <IssueDetailPanel
        repoFullName="acme/treq"
        issueNumber={42}
        onClose={() => {}}
      />,
    );

    await waitFor(() => {
      expect(screen.getByText("world").tagName).toBe("STRONG");
    });
    expect(screen.getByText("code").tagName).toBe("CODE");
    expect(screen.getByText("good").tagName).toBe("EM");
  });

  it("submits a comment with Ctrl+Enter", async () => {
    api.ghCreateIssueComment.mockResolvedValue(undefined);

    render(
      <IssueDetailPanel
        repoFullName="acme/treq"
        issueNumber={42}
        onClose={() => {}}
      />,
    );

    await screen.findByText("world");
    const textarea = screen.getByPlaceholderText(/leave a comment/i);
    await user.type(textarea, "Hello from test");
    await user.keyboard("{Control>}{Enter}{/Control}");

    await waitFor(() => {
      expect(api.ghCreateIssueComment).toHaveBeenCalled();
    });
  });
});

describe("PrDetailPanel draft toggle", () => {
  let user: ReturnType<typeof userEvent.setup>;

  beforeEach(() => {
    user = userEvent.setup();
    api.ghSetPrDraft.mockReset().mockResolvedValue(undefined);
    api.getWorkspaces.mockResolvedValue([]);
    api.openOrCreateWorkspaceFromPr.mockReset();
  });

  it("marks a draft PR ready for review", async () => {
    api.ghViewPr
      .mockResolvedValueOnce(makeDetailPr({ is_draft: true }))
      .mockResolvedValueOnce(makeDetailPr({ is_draft: false }));

    render(
      <PrDetailPanel
        repoPath="/tmp/repo"
        repoFullName="acme/treq"
        prNumber={42}
        onClose={() => {}}
      />,
    );

    expect(await screen.findByText("Draft")).toBeVisible();
    await user.click(screen.getByRole("button", { name: /ready for review/i }));

    await waitFor(() => {
      expect(api.ghSetPrDraft).toHaveBeenCalledWith("acme/treq", 42, false);
    });
    expect(await screen.findByText("Open")).toBeVisible();
    expect(
      screen.getByRole("button", { name: /convert to draft/i }),
    ).toBeVisible();
  });

  it("converts an open PR to draft", async () => {
    api.ghViewPr
      .mockResolvedValueOnce(makeDetailPr({ is_draft: false }))
      .mockResolvedValueOnce(makeDetailPr({ is_draft: true }));

    render(
      <PrDetailPanel
        repoPath="/tmp/repo"
        repoFullName="acme/treq"
        prNumber={42}
        onClose={() => {}}
      />,
    );

    expect(await screen.findByText("Open")).toBeVisible();
    await user.click(screen.getByRole("button", { name: /convert to draft/i }));

    await waitFor(() => {
      expect(api.ghSetPrDraft).toHaveBeenCalledWith("acme/treq", 42, true);
    });
    expect(await screen.findByText("Draft")).toBeVisible();
    expect(
      screen.getByRole("button", { name: /ready for review/i }),
    ).toBeVisible();
  });
});

describe("PrDetailPanel close PR", () => {
  let user: ReturnType<typeof userEvent.setup>;

  beforeEach(() => {
    user = userEvent.setup();
    api.getWorkspaces.mockResolvedValue([]);
    api.ghClosePr.mockReset();
  });

  it("closes the PR on one click, with no confirmation dialog", async () => {
    api.ghViewPr.mockResolvedValue(makeDetailPr({ is_draft: false }));
    api.ghClosePr.mockResolvedValue(undefined);

    render(
      <PrDetailPanel
        repoPath="/tmp/repo"
        repoFullName="acme/treq"
        prNumber={42}
        onClose={() => {}}
      />,
    );

    await user.click(await screen.findByRole("button", { name: /close pr/i }));

    await waitFor(() => {
      expect(api.ghClosePr).toHaveBeenCalledWith("acme/treq", 42);
    });
    expect(screen.queryByRole("dialog")).toBeNull();
  });

  it("shows a loading state on the Close PR button while the request is in flight", async () => {
    let resolveClose: () => void = () => {};
    api.ghViewPr.mockResolvedValue(makeDetailPr({ is_draft: false }));
    api.ghClosePr.mockImplementation(
      () =>
        new Promise<void>((resolve) => {
          resolveClose = resolve;
        }),
    );

    render(
      <PrDetailPanel
        repoPath="/tmp/repo"
        repoFullName="acme/treq"
        prNumber={42}
        onClose={() => {}}
      />,
    );

    await user.click(await screen.findByRole("button", { name: /close pr/i }));

    const closing = await screen.findByRole("button", { name: /closing/i });
    expect(closing).toBeDisabled();
    expect(closing).toHaveAttribute("aria-busy", "true");

    resolveClose();
    await waitFor(() => {
      expect(api.ghClosePr).toHaveBeenCalledWith("acme/treq", 42);
    });
  });
});

describe("PrDetailPanel refreshes the workspace PR status", () => {
  let user: ReturnType<typeof userEvent.setup>;

  beforeEach(() => {
    user = userEvent.setup();
    api.getWorkspaces.mockResolvedValue([]);
    api.ghClosePr.mockReset().mockResolvedValue(undefined);
    api.ghReopenPr.mockReset().mockResolvedValue(undefined);
    api.ghSetPrDraft.mockReset().mockResolvedValue(undefined);
    api.getPrInfoViaGh.mockReset().mockResolvedValue(null);
    api.getPrChecksViaGh.mockReset().mockResolvedValue(null);
  });

  it.each([
    { action: /close pr/i, state: "OPEN", draft: false },
    { action: /reopen pr/i, state: "CLOSED", draft: false },
    { action: /convert to draft/i, state: "OPEN", draft: false },
    { action: /ready for review/i, state: "OPEN", draft: true },
  ])("re-fetches the head branch PR status after $action", async ({
    action,
    state,
    draft,
  }) => {
    api.ghViewPr.mockResolvedValue(makeDetailPr({ state, is_draft: draft }));

    render(
      <PrDetailPanel
        repoPath="/tmp/repo"
        repoFullName="acme/treq"
        prNumber={42}
        onClose={() => {}}
      />,
    );

    await user.click(await screen.findByRole("button", { name: action }));

    await waitFor(() => {
      expect(api.getPrInfoViaGh).toHaveBeenCalledWith("/tmp/repo", "feat");
    });
  });
});

describe("GitHub detail load failures", () => {
  it("shows the gh error when a PR fails to load", async () => {
    api.getWorkspaces.mockResolvedValue([]);
    api.ghViewPr.mockRejectedValue("gh: Could not resolve to a PullRequest");

    render(
      <PrDetailPanel
        repoPath="/tmp/repo"
        repoFullName="acme/treq"
        prNumber={404}
        onClose={() => {}}
      />,
    );

    expect(
      await screen.findByText("Could not load pull request #404"),
    ).toBeVisible();
    expect(
      screen.getByText("gh: Could not resolve to a PullRequest"),
    ).toBeVisible();
  });

  it("shows the gh error when an issue fails to load", async () => {
    api.ghViewIssue.mockRejectedValue(new Error("gh: HTTP 404"));

    render(
      <IssueDetailPanel
        repoFullName="acme/treq"
        issueNumber={404}
        onClose={() => {}}
      />,
    );

    expect(await screen.findByText("Could not load issue #404")).toBeVisible();
    expect(screen.getByText("gh: HTTP 404")).toBeVisible();
  });
});

describe("GitHub detail action failures", () => {
  let user: ReturnType<typeof userEvent.setup>;

  beforeEach(() => {
    user = userEvent.setup();
    api.getWorkspaces.mockResolvedValue([]);
    api.ghViewPr.mockResolvedValue(makeDetailPr({}));
    api.ghViewIssue.mockResolvedValue({
      number: 7,
      title: "Broken thing",
      state: "OPEN",
      url: "https://github.com/acme/treq/issues/7",
      body: null,
      author: { login: "alice" },
      created_at: "2026-01-01T00:00:00Z",
      labels: [],
      comments: null,
    });
    api.ghClosePr.mockReset();
    api.ghCreatePrComment.mockReset();
    api.ghCloseIssue.mockReset();
  });

  function renderPr() {
    render(
      <PrDetailPanel
        repoPath="/tmp/repo"
        repoFullName="acme/treq"
        prNumber={42}
        onClose={() => {}}
      />,
    );
  }

  it("shows the gh error when closing a PR fails", async () => {
    api.ghClosePr.mockRejectedValue(
      "gh: Resource not accessible by integration",
    );
    renderPr();

    await user.click(await screen.findByRole("button", { name: /close pr/i }));

    expect(
      await screen.findByText("Failed to close pull request"),
    ).toBeVisible();
    expect(
      screen.getByText("gh: Resource not accessible by integration"),
    ).toBeVisible();
    expect(screen.getByRole("button", { name: /close pr/i })).toBeEnabled();
  });

  it("keeps the draft and shows the gh error when a PR comment fails", async () => {
    api.ghCreatePrComment.mockRejectedValue("gh: HTTP 502");
    renderPr();

    const textarea = await screen.findByPlaceholderText(/leave a comment/i);
    await user.type(textarea, "Needs a test");
    await user.click(screen.getByRole("button", { name: /^comment$/i }));

    expect(await screen.findByText("Failed to post comment")).toBeVisible();
    expect(screen.getByText("gh: HTTP 502")).toBeVisible();
    expect(textarea).toHaveValue("Needs a test");
  });

  it("shows the gh error when closing an issue fails", async () => {
    api.ghCloseIssue.mockRejectedValue(new Error("gh: issue is locked"));
    render(
      <IssueDetailPanel
        repoFullName="acme/treq"
        issueNumber={7}
        onClose={() => {}}
      />,
    );

    await user.click(
      await screen.findByRole("button", { name: /close issue/i }),
    );

    expect(await screen.findByText("Failed to close issue")).toBeVisible();
    expect(screen.getByText("gh: issue is locked")).toBeVisible();
  });
});

describe("CreatePrForm", () => {
  let user: ReturnType<typeof userEvent.setup>;

  beforeEach(() => {
    user = userEvent.setup();
    api.ghCreatePr.mockReset().mockResolvedValue(7);
  });

  async function fillForm() {
    await user.type(screen.getByPlaceholderText("Title"), "Add thing");
    await user.type(screen.getByPlaceholderText("Head branch"), "feat/thing");
    const base = screen.getByPlaceholderText("Base branch");
    await user.clear(base);
    await user.type(base, "main");
  }

  it("creates a ready-for-review PR by default", async () => {
    render(
      <CreatePrForm
        repoPath="/tmp/repo"
        repoFullName="acme/treq"
        onSuccess={() => {}}
        onCancel={() => {}}
      />,
    );

    await fillForm();
    await user.click(
      screen.getByRole("button", { name: /^create pull request$/i }),
    );

    await waitFor(() => {
      expect(api.ghCreatePr).toHaveBeenCalledWith(
        "acme/treq",
        "Add thing",
        "",
        "main",
        "feat/thing",
        false,
      );
    });
  });

  it("creates a draft PR after choosing draft from the dropdown", async () => {
    const onSuccess = vi.fn();
    render(
      <CreatePrForm
        repoPath="/tmp/repo"
        repoFullName="acme/treq"
        onSuccess={onSuccess}
        onCancel={() => {}}
      />,
    );

    await fillForm();
    expect(screen.queryByRole("switch")).toBeNull();
    await user.click(
      screen.getByRole("button", { name: /pull request type/i }),
    );
    await user.click(
      await screen.findByRole("menuitemradio", {
        name: /create draft pull request/i,
      }),
    );
    expect(api.ghCreatePr).not.toHaveBeenCalled();
    await user.click(
      await screen.findByRole("button", { name: /^draft pull request$/i }),
    );

    await waitFor(() => {
      expect(api.ghCreatePr).toHaveBeenCalledWith(
        "acme/treq",
        "Add thing",
        "",
        "main",
        "feat/thing",
        true,
      );
    });
    expect(onSuccess).toHaveBeenCalledWith(7);
  });

  it("shows the gh error message without an Error prefix", async () => {
    api.ghCreatePr.mockRejectedValue(new Error("gh: head branch not found"));
    render(
      <CreatePrForm
        repoPath="/tmp/repo"
        repoFullName="acme/treq"
        onSuccess={() => {}}
        onCancel={() => {}}
      />,
    );

    await fillForm();
    await user.click(
      screen.getByRole("button", { name: /^create pull request$/i }),
    );

    expect(await screen.findByText("gh: head branch not found")).toBeVisible();
  });
});
