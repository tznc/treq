import { openUrl } from "@tauri-apps/plugin-opener";
import userEvent from "@testing-library/user-event";
import { beforeEach, describe, expect, it, vi } from "vitest";
import { render, screen } from "../../test/test-utils";
import { GitHubIntegrationSettings } from "./GitHubIntegrationSettings";

const auth = vi.hoisted(() => ({
  user: { id: "user-1" } as object | null,
  session: { access_token: "token" } as object | null,
  loading: false,
  // The app last read Pro, but the server knows the subscription ended.
  subscription: { plan: "pro", status: "active" } as {
    plan: string;
    status: string;
  } | null,
  signIn: vi.fn(),
}));
const mutateAsync = vi.hoisted(() => vi.fn());

vi.mock("../stores/authStore", async (importOriginal) => {
  const actual = await importOriginal<typeof import("../stores/authStore")>();
  const { createAuthStoreMock } = await import(
    "../../test/mocks/zustandAuthStore"
  );
  return { ...actual, useAuthStore: createAuthStoreMock(auth) };
});
vi.mock("../lib/features", async (importOriginal) => {
  const actual = await importOriginal<typeof import("../lib/features")>();
  return { ...actual, FEATURES: { ...actual.FEATURES, mergeQueue: true } };
});
vi.mock("../lib/supabase", async (importOriginal) => {
  const actual = await importOriginal<typeof import("../lib/supabase")>();
  return {
    ...actual,
    WEB_URL: "http://localhost:3001",
    supabase: {
      from: () => ({
        select: async () => ({
          data: [
            {
              id: 1,
              full_name: "acme/app",
              private: false,
              default_branch: "main",
              installation_id: 10,
            },
          ],
          error: null,
        }),
      }),
    },
  };
});
vi.mock("../hooks/useMergeQueueStatus", () => ({
  useGitRemoteInfo: () => ({ data: { full_name: "acme/app" } }),
  useMergeQueueEnabled: () => ({ data: false, isLoading: false }),
  useSetMergeQueueEnabled: () => ({ mutateAsync, isPending: false }),
}));

describe("merge queue setting", () => {
  beforeEach(() => {
    mutateAsync.mockReset();
    vi.mocked(openUrl).mockReset();
  });

  it("swaps the enable button for Upgrade to Pro when the server answers pro_required", async () => {
    // What supabase-js returns for set_merge_queue_enabled's PT402.
    mutateAsync.mockRejectedValue({
      code: "PT402",
      message: "The merge queue needs Pro",
      details: null,
      hint: "pro_required",
    });
    const user = userEvent.setup();
    render(<GitHubIntegrationSettings repoPath="/repo" />);

    await user.click(
      await screen.findByRole("button", { name: "Enable merge queue" }),
    );

    expect(
      await screen.findByText("Upgrade to Pro to use the merge queue."),
    ).toBeVisible();
    expect(
      screen.queryByRole("button", { name: "Enable merge queue" }),
    ).not.toBeInTheDocument();
    expect(
      screen.queryByText("The merge queue needs Pro"),
    ).not.toBeInTheDocument();
    await user.click(screen.getByRole("button", { name: "Upgrade to Pro" }));
    expect(openUrl).toHaveBeenCalledWith("http://localhost:3001/dashboard");
  });

  it("shows any other failure as an error", async () => {
    mutateAsync.mockRejectedValue(new Error("Repository is not linked"));
    const user = userEvent.setup();
    render(<GitHubIntegrationSettings repoPath="/repo" />);

    await user.click(
      await screen.findByRole("button", { name: "Enable merge queue" }),
    );

    expect(await screen.findByText("Repository is not linked")).toBeVisible();
    expect(
      screen.getByRole("button", { name: "Enable merge queue" }),
    ).toBeInTheDocument();
  });
});
