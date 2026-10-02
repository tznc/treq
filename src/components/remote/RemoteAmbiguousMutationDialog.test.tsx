import userEvent from "@testing-library/user-event";
import { beforeEach, describe, expect, it, vi } from "vitest";
import { render, screen } from "../../../test/test-utils";
import { scheduleRefreshWorkspaceChanges } from "../../lib/change-file-drag";
import { remoteActionKeys } from "../../lib/remote-idempotency";
import { useRemoteMutationFeedback } from "../../lib/remote-mutation-ui";
import { invalidateQueries } from "../../lib/swr-cache";
import { RemoteAmbiguousMutationDialog } from "./RemoteAmbiguousMutationDialog";

vi.mock("../../lib/swr-cache", async (importOriginal) => ({
  ...(await importOriginal<typeof import("../../lib/swr-cache")>()),
  invalidateQueries: vi.fn(() => Promise.resolve()),
}));
vi.mock("../../lib/change-file-drag", () => ({
  scheduleRefreshWorkspaceChanges: vi.fn(),
}));

describe("RemoteAmbiguousMutationDialog", () => {
  beforeEach(() => {
    useRemoteMutationFeedback.getState().report({
      status: "ambiguous",
      reason: "connection reset",
    });
    vi.mocked(invalidateQueries).mockClear();
    vi.mocked(scheduleRefreshWorkspaceChanges).mockClear();
  });

  it("refreshes the repository's remote data and closes on Refresh", async () => {
    const user = userEvent.setup();
    const pending = remoteActionKeys.keyFor("GitPush", ["/r"], "repo");
    render(<RemoteAmbiguousMutationDialog />);
    expect(screen.getByTestId("remote-ambiguous-reason")).toHaveTextContent(
      "connection reset",
    );

    await user.click(screen.getByRole("button", { name: "Refresh" }));

    expect(invalidateQueries).toHaveBeenCalled();
    expect(scheduleRefreshWorkspaceChanges).toHaveBeenCalled();
    // Refresh may fail while disconnected, so the retry keeps its key.
    expect(remoteActionKeys.keyFor("GitPush", ["/r"], "repo")).toBe(pending);
    remoteActionKeys.release();
    expect(useRemoteMutationFeedback.getState().ambiguousReason).toBeNull();
  });

  it("closes without refreshing on Dismiss", async () => {
    const user = userEvent.setup();
    render(<RemoteAmbiguousMutationDialog />);

    await user.click(screen.getByRole("button", { name: "Dismiss" }));

    expect(invalidateQueries).not.toHaveBeenCalled();
    expect(useRemoteMutationFeedback.getState().ambiguousReason).toBeNull();
  });
});
