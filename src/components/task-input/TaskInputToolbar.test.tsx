import { describe, expect, it, vi } from "vitest";
import userEvent from "@testing-library/user-event";
import { render, screen } from "../../../test/test-utils";
import { TaskInputToolbar } from "./TaskInputToolbar";

describe("TaskInputToolbar add menu", () => {
  it.each([
    "File",
    "Attach",
  ])("opens the %s action from Add", async (action) => {
    const user = userEvent.setup();
    const onOpenFilePicker = vi.fn();
    const onAttachFromFinder = vi.fn();
    render(
      <TaskInputToolbar
        isEmpty={true}
        submitting={false}
        selectedAgent="claude"
        configuredDefaultAgent="claude"
        saveAsRepoDefault={false}
        showSaveAsRepoDefault={false}
        onOpenFilePicker={onOpenFilePicker}
        onAttachFromFinder={onAttachFromFinder}
        onAgentChange={vi.fn()}
        onSaveAsRepoDefaultChange={vi.fn()}
        onSubmit={vi.fn()}
      />,
    );
    await user.click(screen.getByRole("button", { name: "Add" }));
    await user.click(await screen.findByRole("menuitem", { name: action }));
    expect(onOpenFilePicker).toHaveBeenCalledTimes(action === "File" ? 1 : 0);
    expect(onAttachFromFinder).toHaveBeenCalledTimes(
      action === "Attach" ? 1 : 0,
    );
  });
});
