import userEvent from "@testing-library/user-event";
import { beforeEach, describe, expect, it, vi } from "vitest";
import { IssueDetailPanel } from "./IssueDetail";
import { render, screen } from "../../../test/test-utils";

const api = vi.hoisted(() => ({
  ghViewIssue: vi.fn(),
}));

vi.mock("../../lib/api", async (importOriginal) => {
  const original = await importOriginal<typeof import("../../lib/api")>();
  return {
    ...original,
    ghViewIssue: api.ghViewIssue,
  };
});

describe("IssueDetailPanel Agent prompt", () => {
  let user: ReturnType<typeof userEvent.setup>;

  beforeEach(() => {
    user = userEvent.setup();
    api.ghViewIssue.mockResolvedValue({
      number: 42,
      title: "Fix markdown",
      state: "OPEN",
      url: "https://github.com/acme/treq/issues/42",
      body: "Hello **world**",
      author: { login: "alice" },
      created_at: "2026-01-01T00:00:00Z",
      labels: [],
      comments: null,
    });
  });

  it("calls onStartPrompt with the issue when Agent is clicked", async () => {
    const onStartPrompt = vi.fn();
    render(
      <IssueDetailPanel
        repoFullName="acme/treq"
        issueNumber={42}
        onClose={() => {}}
        onStartPrompt={onStartPrompt}
      />,
    );

    await screen.findByText("Fix markdown");
    await user.click(screen.getByRole("button", { name: /^agent/i }));

    expect(onStartPrompt).toHaveBeenCalledWith({
      number: 42,
      url: "https://github.com/acme/treq/issues/42",
      title: "Fix markdown",
    });
  });
});
