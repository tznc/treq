import { render, screen, waitFor } from "../../test/test-utils";
import userEvent from "@testing-library/user-event";
import { beforeEach, describe, expect, it, vi } from "vitest";
import { openUrl } from "@tauri-apps/plugin-opener";
import { GitHubIntegrationSettings } from "./GitHubIntegrationSettings";

const auth = vi.hoisted(() => ({
  user: { id: "user-1" } as object | null,
  session: { access_token: "token" } as object | null,
  loading: false,
  subscription: null as { plan: string; status: string } | null,
  signIn: vi.fn(),
}));
const query = vi.hoisted(() => vi.fn());
const mockFeatures = vi.hoisted(() => ({ mergeQueue: false }));

vi.mock("../lib/features", async (importOriginal) => {
  const actual = await importOriginal<typeof import("../lib/features")>();
  return {
    ...actual,
    FEATURES: Object.assign(mockFeatures, {
      ...actual.FEATURES,
      ...mockFeatures,
    }),
  };
});

vi.mock("../stores/authStore", async (importOriginal) => {
  const actual = await importOriginal<typeof import("../stores/authStore")>();
  const { createAuthStoreMock } = await import(
    "../../test/mocks/zustandAuthStore"
  );
  return { ...actual, useAuthStore: createAuthStoreMock(auth) };
});
vi.mock("../lib/supabase", async (importOriginal) => {
  const original = await importOriginal<typeof import("../lib/supabase")>();
  return {
    ...original,
    supabase: { from: () => ({ select: query }) },
    WEB_URL: "http://localhost:3001",
  };
});

const repositories = [
  {
    id: 1,
    full_name: "acme/public",
    private: false,
    default_branch: "main",
    installation_id: 10,
  },
  {
    id: 2,
    full_name: "acme/private",
    private: true,
    default_branch: "trunk",
    installation_id: 10,
  },
];

describe("GitHubIntegrationSettings", () => {
  beforeEach(() => {
    auth.user = { id: "user-1" };
    auth.session = { access_token: "token" };
    auth.loading = false;
    auth.subscription = null;
    auth.signIn.mockReset();
    mockFeatures.mergeQueue = false;
    query.mockReset();
    vi.mocked(openUrl).mockReset();
  });

  it("asks signed-out users to sign in", async () => {
    auth.user = null;
    auth.session = null;
    render(<GitHubIntegrationSettings />);
    await userEvent
      .setup()
      .click(screen.getByRole("button", { name: /sign in with browser/i }));
    expect(auth.signIn).toHaveBeenCalled();
    expect(query).not.toHaveBeenCalled();
  });

  it("shows only public repositories for Free users", async () => {
    query.mockResolvedValue({ data: repositories, error: null });
    render(<GitHubIntegrationSettings />);
    expect(await screen.findByText("Public repositories")).toBeVisible();
    expect(screen.getByText("acme/public")).toBeVisible();
    expect(screen.queryByText("acme/private")).not.toBeInTheDocument();
  });

  it("shows public and private repositories for active Pro users", async () => {
    auth.subscription = { plan: "pro", status: "active" };
    query.mockResolvedValue({ data: repositories, error: null });
    render(<GitHubIntegrationSettings />);
    expect(await screen.findByText("All repositories")).toBeVisible();
    expect(screen.getByText("acme/public")).toBeVisible();
    expect(screen.getByText("acme/private")).toBeVisible();
  });

  it("handles loading, failure, and empty repository states", async () => {
    let resolveQuery!: (value: unknown) => void;
    query.mockReturnValue(
      new Promise((resolve) => {
        resolveQuery = resolve;
      }),
    );
    const view = render(<GitHubIntegrationSettings />);
    expect(screen.getByText(/loading github repositories/i)).toBeVisible();
    resolveQuery({ data: null, error: new Error("denied") });
    expect(
      await screen.findByText(/could not load github repositories/i),
    ).toBeVisible();

    query.mockResolvedValue({ data: [], error: null });
    view.unmount();
    render(<GitHubIntegrationSettings />);
    expect(
      await screen.findByText(/no enabled github repositories/i),
    ).toBeVisible();
  });

  it("hides the merge queue setting when the mergeQueue flag is off", async () => {
    auth.subscription = { plan: "pro", status: "active" };
    query.mockResolvedValue({ data: repositories, error: null });
    render(<GitHubIntegrationSettings />);
    expect(await screen.findByText("acme/public")).toBeVisible();
    expect(screen.queryByTestId("merge-queue-setting")).not.toBeInTheDocument();
    expect(screen.queryByText("Merge queue")).not.toBeInTheDocument();
  });

  it("shows the merge queue setting when the mergeQueue flag is on", async () => {
    mockFeatures.mergeQueue = true;
    query.mockResolvedValue({ data: repositories, error: null });
    render(<GitHubIntegrationSettings />);
    expect(await screen.findByTestId("merge-queue-setting")).toBeVisible();
    expect(screen.getByText("Merge queue")).toBeVisible();
  });

  it("opens the integrations dashboard to manage GitHub", async () => {
    query.mockResolvedValue({ data: [], error: null });
    render(<GitHubIntegrationSettings />);
    await userEvent
      .setup()
      .click(await screen.findByRole("button", { name: /manage github/i }));
    await waitFor(() =>
      expect(openUrl).toHaveBeenCalledWith(
        "http://localhost:3001/dashboard?tab=integrations",
      ),
    );
  });
});
