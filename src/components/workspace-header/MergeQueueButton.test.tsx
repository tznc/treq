import { openUrl } from "@tauri-apps/plugin-opener";
import userEvent from "@testing-library/user-event";
import { beforeEach, describe, expect, it, vi } from "vitest";
import { render, screen, waitFor } from "../../../test/test-utils";
import { useToastStore } from "../../stores/toastStore";
import { MergeQueueButton } from "./MergeQueueButton";

const enqueue = vi.hoisted(() => vi.fn());

vi.mock("../../lib/features", async (importOriginal) => {
  const actual = await importOriginal<typeof import("../../lib/features")>();
  return { ...actual, FEATURES: { ...actual.FEATURES, mergeQueue: true } };
});
vi.mock("../../lib/supabase", async (importOriginal) => {
  const actual = await importOriginal<typeof import("../../lib/supabase")>();
  return { ...actual, WEB_URL: "http://localhost:3001" };
});
vi.mock("../../hooks/useMergeQueueStatus", () => ({
  useMergeQueueEnabled: () => ({ data: true }),
  useMergeQueueStatus: () => ({ data: null }),
  useEnqueueWorkspace: () => ({
    enqueue: { mutateAsync: enqueue, isPending: false },
    dequeue: { mutateAsync: vi.fn(), isPending: false },
  }),
}));

describe("MergeQueueButton", () => {
  beforeEach(() => {
    enqueue.mockReset();
    vi.mocked(openUrl).mockReset();
    useToastStore.setState({ toasts: [] });
  });

  it("offers Upgrade to Pro when enqueue-workspace answers pro_required", async () => {
    enqueue.mockRejectedValue(
      Object.assign(new Error("Edge Function returned a non-2xx status code"), {
        context: new Response(
          JSON.stringify({
            error: "The merge queue needs Pro.",
            code: "pro_required",
          }),
          { status: 402 },
        ),
      }),
    );
    const user = userEvent.setup();
    render(<MergeQueueButton repoPath="/repo" branchName="feat/a" />);

    await user.click(screen.getByRole("button", { name: /Add to Queue/ }));

    await waitFor(() =>
      expect(useToastStore.getState().toasts).toHaveLength(1),
    );
    const [toast] = useToastStore.getState().toasts;
    expect(toast).toMatchObject({
      title: "The merge queue needs Pro",
      type: "info",
      action: { label: "Upgrade to Pro" },
    });
    toast.action!.onClick();
    expect(openUrl).toHaveBeenCalledWith("http://localhost:3001/dashboard");
  });

  it("reports other failures as a queue error", async () => {
    enqueue.mockRejectedValue(
      new Error("No open PR found for branch 'feat/a'"),
    );
    const user = userEvent.setup();
    render(<MergeQueueButton repoPath="/repo" branchName="feat/a" />);

    await user.click(screen.getByRole("button", { name: /Add to Queue/ }));

    await waitFor(() =>
      expect(useToastStore.getState().toasts[0]).toMatchObject({
        title: "Queue error",
        type: "error",
      }),
    );
  });
});
