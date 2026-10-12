import userEvent from "@testing-library/user-event";
import { beforeEach, describe, expect, it, vi } from "vitest";
import { CreateIssueForm, IssueDetailPanel } from "./IssueDetail";
import { render, screen, waitFor } from "../../../test/test-utils";

const api = vi.hoisted(() => ({
  ghViewIssue: vi.fn(),
  ghCloseIssue: vi.fn(),
  ghReopenIssue: vi.fn(),
  ghEditIssue: vi.fn(),
  ghCreateIssue: vi.fn(),
}));

vi.mock("../../lib/api", async (importOriginal) => {
  const original = await importOriginal<typeof import("../../lib/api")>();
  return { ...original, ...api };
});

function makeIssue({
  state = "OPEN",
  title = "Broken thing",
  body = null,
}: {
  state?: string;
  title?: string;
  body?: string | null;
} = {}) {
  return {
    number: 7,
    title,
    state,
    url: "https://github.com/acme/treq/issues/7",
    body,
    author: { login: "alice" },
    labels: [],
    created_at: "2026-01-01T00:00:00Z",
    updated_at: "2026-01-01T00:00:00Z",
    comments: null,
  };
}

function renderIssue() {
  render(
    <IssueDetailPanel
      repoFullName="acme/treq"
      issueNumber={7}
      onClose={() => {}}
    />,
  );
}

describe("IssueDetailPanel close issue", () => {
  let user: ReturnType<typeof userEvent.setup>;

  beforeEach(() => {
    user = userEvent.setup();
    for (const fn of Object.values(api)) fn.mockReset();
  });

  it("closes the issue as completed on one click", async () => {
    let closed = false;
    api.ghViewIssue.mockImplementation(async () =>
      makeIssue({ state: closed ? "CLOSED" : "OPEN" }),
    );
    api.ghCloseIssue.mockImplementation(async () => {
      closed = true;
    });
    renderIssue();

    await user.click(
      await screen.findByRole("button", { name: /^close issue$/i }),
    );

    expect(
      await screen.findByRole("button", { name: /reopen issue/i }),
    ).toBeVisible();
    expect(api.ghCloseIssue).toHaveBeenCalledWith("acme/treq", 7, "completed");
    expect(screen.queryByRole("dialog")).toBeNull();
  });

  it("closes the issue as not planned from the close options", async () => {
    api.ghViewIssue.mockResolvedValue(makeIssue({ state: "OPEN" }));
    api.ghCloseIssue.mockResolvedValue(undefined);
    renderIssue();

    await user.click(
      await screen.findByRole("button", { name: /close options/i }),
    );
    await user.click(
      await screen.findByRole("menuitem", { name: /close as not planned/i }),
    );

    await waitFor(() =>
      expect(api.ghCloseIssue).toHaveBeenCalledWith(
        "acme/treq",
        7,
        "not planned",
      ),
    );
  });

  it("shows a loading state on Close issue while the request is in flight", async () => {
    let resolveClose: () => void = () => {};
    api.ghViewIssue.mockResolvedValue(makeIssue({ state: "OPEN" }));
    api.ghCloseIssue.mockImplementation(
      () =>
        new Promise<void>((resolve) => {
          resolveClose = resolve;
        }),
    );
    renderIssue();

    await user.click(
      await screen.findByRole("button", { name: /^close issue$/i }),
    );

    const closing = await screen.findByRole("button", { name: /closing/i });
    expect(closing).toBeDisabled();
    expect(closing).toHaveAttribute("aria-busy", "true");
    expect(
      screen.getByRole("button", { name: /close options/i }),
    ).toBeDisabled();

    resolveClose();
    await waitFor(() =>
      expect(
        screen.getByRole("button", { name: /^close issue$/i }),
      ).toBeEnabled(),
    );
  });

  it("shows a loading state on Reopen issue while the request is in flight", async () => {
    let resolveReopen: () => void = () => {};
    api.ghViewIssue.mockResolvedValue(makeIssue({ state: "CLOSED" }));
    api.ghReopenIssue.mockImplementation(
      () =>
        new Promise<void>((resolve) => {
          resolveReopen = resolve;
        }),
    );
    renderIssue();

    await user.click(
      await screen.findByRole("button", { name: /reopen issue/i }),
    );

    const reopening = await screen.findByRole("button", {
      name: /reopening/i,
    });
    expect(reopening).toBeDisabled();
    expect(reopening).toHaveAttribute("aria-busy", "true");
    resolveReopen();
    await waitFor(() =>
      expect(api.ghReopenIssue).toHaveBeenCalledWith("acme/treq", 7),
    );
  });
});

describe("IssueDetailPanel edit issue", () => {
  let user: ReturnType<typeof userEvent.setup>;
  let current: ReturnType<typeof makeIssue>;

  beforeEach(() => {
    user = userEvent.setup();
    for (const fn of Object.values(api)) fn.mockReset();
    current = makeIssue({ title: "Old title", body: "Old body" });
    api.ghViewIssue.mockImplementation(async () => current);
    api.ghEditIssue.mockImplementation(
      async (...args: [string, number, string, string]) => {
        current = makeIssue({ title: args[2], body: args[3] });
      },
    );
  });

  it("saves a new title and body", async () => {
    renderIssue();

    await user.click(
      await screen.findByRole("button", { name: /edit issue/i }),
    );
    const title = screen.getByRole("textbox", { name: /issue title/i });
    const body = screen.getByRole("textbox", { name: /issue description/i });
    expect(title).toHaveValue("Old title");
    expect(body).toHaveValue("Old body");

    await user.clear(title);
    await user.type(title, "New title");
    await user.clear(body);
    await user.type(body, "New body");
    await user.click(screen.getByRole("button", { name: /^save$/i }));

    expect(
      await screen.findByRole("heading", { name: "New title" }),
    ).toBeVisible();
    expect(await screen.findByText("New body")).toBeVisible();
    expect(api.ghEditIssue).toHaveBeenCalledWith(
      "acme/treq",
      7,
      "New title",
      "New body",
    );
    expect(
      screen.queryByRole("textbox", { name: /issue title/i }),
    ).not.toBeInTheDocument();
  });

  it("starts from an empty description when the issue has no body", async () => {
    current = makeIssue({ title: "Old title", body: null });
    renderIssue();

    await user.click(
      await screen.findByRole("button", { name: /edit issue/i }),
    );

    expect(
      screen.getByRole("textbox", { name: /issue description/i }),
    ).toHaveValue("");
  });

  it("discards edits on Cancel", async () => {
    renderIssue();

    await user.click(
      await screen.findByRole("button", { name: /edit issue/i }),
    );
    const title = screen.getByRole("textbox", { name: /issue title/i });
    await user.clear(title);
    await user.type(title, "Throwaway");
    await user.click(screen.getByRole("button", { name: /^cancel$/i }));

    expect(screen.getByRole("heading", { name: "Old title" })).toBeVisible();
    expect(api.ghEditIssue).not.toHaveBeenCalled();

    await user.click(screen.getByRole("button", { name: /edit issue/i }));
    expect(screen.getByRole("textbox", { name: /issue title/i })).toHaveValue(
      "Old title",
    );
  });

  it("disables Save while the title is empty", async () => {
    renderIssue();

    await user.click(
      await screen.findByRole("button", { name: /edit issue/i }),
    );
    await user.clear(screen.getByRole("textbox", { name: /issue title/i }));

    expect(screen.getByRole("button", { name: /^save$/i })).toBeDisabled();
  });

  it("keeps the edits and shows the gh error when saving fails", async () => {
    api.ghEditIssue.mockRejectedValue("gh: HTTP 403: Must have push access");
    renderIssue();

    await user.click(
      await screen.findByRole("button", { name: /edit issue/i }),
    );
    const title = screen.getByRole("textbox", { name: /issue title/i });
    await user.clear(title);
    await user.type(title, "New title");
    await user.click(screen.getByRole("button", { name: /^save$/i }));

    expect(await screen.findByText("Failed to update issue")).toBeVisible();
    expect(
      screen.getByText("gh: HTTP 403: Must have push access"),
    ).toBeVisible();
    expect(title).toHaveValue("New title");
    expect(screen.getByRole("button", { name: /^save$/i })).toBeEnabled();
  });
});

describe("CreateIssueForm", () => {
  let user: ReturnType<typeof userEvent.setup>;

  beforeEach(() => {
    user = userEvent.setup();
    api.ghCreateIssue.mockReset();
  });

  function renderForm(onSuccess = vi.fn()) {
    render(
      <CreateIssueForm
        repoFullName="acme/treq"
        onSuccess={onSuccess}
        onCancel={() => {}}
      />,
    );
    return onSuccess;
  }

  it("creates the issue and hands back its number", async () => {
    api.ghCreateIssue.mockResolvedValue(12);
    const onSuccess = renderForm();

    await user.type(screen.getByPlaceholderText("Title"), "Crash on save");
    await user.type(
      screen.getByPlaceholderText(/description/i),
      "Steps to reproduce",
    );
    await user.click(screen.getByRole("button", { name: /submit issue/i }));

    await waitFor(() => expect(onSuccess).toHaveBeenCalledWith(12));
    expect(api.ghCreateIssue).toHaveBeenCalledWith(
      "acme/treq",
      "Crash on save",
      "Steps to reproduce",
    );
  });

  it("shows the gh error message without an Error prefix", async () => {
    api.ghCreateIssue.mockRejectedValue(
      new Error("gh: could not create issue: HTTP 410"),
    );
    renderForm();

    await user.type(screen.getByPlaceholderText("Title"), "Crash on save");
    await user.click(screen.getByRole("button", { name: /submit issue/i }));

    expect(
      await screen.findByText("gh: could not create issue: HTTP 410"),
    ).toBeVisible();
    expect(screen.queryByText(/^Error:/)).not.toBeInTheDocument();
    expect(screen.getByRole("button", { name: /submit issue/i })).toBeEnabled();
  });

  it("shows a string rejection from the Tauri command as is", async () => {
    api.ghCreateIssue.mockRejectedValue("gh exited with error: HTTP 404");
    renderForm();

    await user.type(screen.getByPlaceholderText("Title"), "Crash on save");
    await user.click(screen.getByRole("button", { name: /submit issue/i }));

    expect(
      await screen.findByText("gh exited with error: HTTP 404"),
    ).toBeVisible();
  });
});
