// @include-serial
import { beforeEach, describe, expect, it } from "vitest";
import { screen, waitFor, within } from "../../test-utils";
import userEvent from "@testing-library/user-event";
import {
  addSingleReviewComment,
  clickChangedFile,
  openReviewTab,
  setupEditableComment,
  setupWorkspaceWithDiff,
  startEditingComment,
} from "./comments-helpers";

describe("Inline comments display", () => {
  let user: ReturnType<typeof userEvent.setup>;

  beforeEach(() => {
    user = userEvent.setup();
  });

  it("should display inline comment on the correct line when line numbers are high", async () => {
    await setupWorkspaceWithDiff(
      "feat/inline-high-lines",
      "context line 1\ncontext line 2\nnew line at 102\ncontext line 3\n",
    );
    await openReviewTab(user, "feat/inline-high-lines");

    await clickChangedFile("test.txt");
    await addSingleReviewComment(user, "Review comment on line 102");

    await screen.findByText("Review comment on line 102");
  });
});

describe("Inline comment editing — save", () => {
  let user: ReturnType<typeof userEvent.setup>;

  beforeEach(() => {
    user = userEvent.setup();
  });

  it("enters edit mode when clicking an inline comment", async () => {
    await setupEditableComment(
      user,
      "feat/comment-edit-open",
      "Original comment text",
    );

    const textarea = await startEditingComment("Original comment text");
    const editForm = textarea.parentElement;
    expect(editForm).toBeTruthy();

    expect(
      within(editForm!).getByRole("button", { name: /^save$/i }),
    ).toBeTruthy();
    expect(
      within(editForm!).getByRole("button", { name: /^cancel$/i }),
    ).toBeTruthy();
    expect(
      within(editForm!).getByRole("button", { name: /^discard$/i }),
    ).toBeTruthy();
  });

  it("updates inline comment text when saving", async () => {
    await setupEditableComment(
      user,
      "feat/comment-edit-save",
      "Original comment text",
    );

    const textarea = await startEditingComment("Original comment text");
    await user.clear(textarea);
    await user.type(textarea, "Updated comment text");
    const editForm = textarea.parentElement;
    expect(editForm).toBeTruthy();
    await user.click(
      within(editForm!).getByRole("button", { name: /^save$/i }),
    );

    await screen.findByText("Updated comment text");
    expect(screen.queryByDisplayValue("Updated comment text")).toBeNull();
    expect(screen.queryByText("Original comment text")).toBeNull();
  });
});

describe("Inline comment editing — cancel", () => {
  let user: ReturnType<typeof userEvent.setup>;

  beforeEach(() => {
    user = userEvent.setup();
  });

  it("restores the original inline comment when canceling edit", async () => {
    await setupEditableComment(
      user,
      "feat/comment-edit-cancel",
      "Original comment text",
    );

    const textarea = await startEditingComment("Original comment text");
    await user.clear(textarea);
    await user.type(textarea, "Modified but not saved");
    const editForm = textarea.parentElement;
    expect(editForm).toBeTruthy();
    await user.click(
      within(editForm!).getByRole("button", { name: /^cancel$/i }),
    );

    await screen.findByText("Original comment text");
    expect(screen.queryByText("Modified but not saved")).toBeNull();
  });

  it("cancels inline comment editing with Escape", async () => {
    await setupEditableComment(
      user,
      "feat/comment-edit-escape",
      "Original comment text",
    );

    const textarea = await startEditingComment("Original comment text");
    await user.clear(textarea);
    await user.type(textarea, "Will be discarded");
    await user.keyboard("{Escape}");

    await screen.findByText("Original comment text");
    expect(screen.queryByDisplayValue("Will be discarded")).toBeNull();
  });
});

describe("Inline comment editing — empty save and discard", () => {
  let user: ReturnType<typeof userEvent.setup>;

  beforeEach(() => {
    user = userEvent.setup();
  });

  it("disables saving when the edited inline comment is empty", async () => {
    await setupEditableComment(
      user,
      "feat/comment-edit-empty",
      "Original comment text",
    );

    const textarea = await startEditingComment("Original comment text");
    await user.clear(textarea);

    const editForm = textarea.parentElement;
    expect(editForm).toBeTruthy();
    expect(
      within(editForm!).getByRole("button", { name: /^save$/i }),
    ).toBeDisabled();
  });

  it("discards the inline comment from edit mode", async () => {
    await setupEditableComment(
      user,
      "feat/comment-edit-discard",
      "Original comment text",
    );

    const textarea = await startEditingComment("Original comment text");
    const editForm = textarea.parentElement;
    expect(editForm).toBeTruthy();

    await user.click(
      within(editForm!).getByRole("button", { name: /^discard$/i }),
    );

    await waitFor(() => {
      expect(screen.queryByText("Original comment text")).toBeNull();
    });
  });
});
