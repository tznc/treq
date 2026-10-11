import { openUrl } from "@tauri-apps/plugin-opener";
import userEvent from "@testing-library/user-event";
import { beforeEach, describe, expect, it, vi } from "vitest";
import { render, screen, waitFor } from "../../test/test-utils";
import { useToastStore } from "../stores/toastStore";
import { LinearIntegrationSettings } from "./LinearIntegrationSettings";

const auth = vi.hoisted(() => ({
  user: { id: "user-1" },
  session: { access_token: "token" },
  loading: false,
  subscription: { plan: "pro", status: "active" },
}));
const invoke = vi.hoisted(() => vi.fn());

vi.mock("../stores/authStore", async (importOriginal) => {
  const actual = await importOriginal<typeof import("../stores/authStore")>();
  const { createAuthStoreMock } = await import(
    "../../test/mocks/zustandAuthStore"
  );
  return { ...actual, useAuthStore: createAuthStoreMock(auth) };
});
vi.mock("../lib/features", async (importOriginal) => {
  const actual = await importOriginal<typeof import("../lib/features")>();
  return { ...actual, FEATURES: { ...actual.FEATURES, pro: true } };
});
vi.mock("../lib/supabase", async (importOriginal) => {
  const actual = await importOriginal<typeof import("../lib/supabase")>();
  return {
    ...actual,
    WEB_URL: "http://localhost:3001",
    supabase: { functions: { invoke } },
  };
});
vi.mock("../lib/api-linear", () => ({
  getLinearApiKey: vi.fn(async () => null),
  setLinearApiKey: vi.fn(),
  getLinearAutoKickoffLabel: vi.fn(async () => null),
  setLinearAutoKickoffLabel: vi.fn(),
  linearStartAutoKickoffPolling: vi.fn(),
}));

describe("Linear OAuth connect", () => {
  beforeEach(() => {
    invoke.mockReset();
    vi.mocked(openUrl).mockReset();
    useToastStore.setState({ toasts: [] });
  });

  it("offers Upgrade to Pro and the API key path when the server answers pro_required", async () => {
    invoke.mockResolvedValue({
      data: null,
      error: Object.assign(
        new Error("Edge Function returned a non-2xx status code"),
        {
          context: new Response(
            JSON.stringify({
              error: "Linear OAuth needs Pro.",
              code: "pro_required",
            }),
            { status: 402 },
          ),
        },
      ),
    });
    const user = userEvent.setup();
    render(<LinearIntegrationSettings repoPath="/repo" />);

    await user.click(screen.getByRole("button", { name: /Connect via OAuth/ }));

    await waitFor(() =>
      expect(useToastStore.getState().toasts).toHaveLength(1),
    );
    const [toast] = useToastStore.getState().toasts;
    expect(toast).toMatchObject({
      title: "Linear OAuth needs Pro",
      type: "info",
      action: { label: "Upgrade to Pro" },
    });
    expect(toast.description).toMatch(/API key/);
    toast.action!.onClick();
    expect(openUrl).toHaveBeenCalledWith("http://localhost:3001/dashboard");
  });

  it("opens the authorization URL the server returns", async () => {
    invoke.mockResolvedValue({
      data: {
        authorization_url: "https://linear.app/oauth/authorize?state=abc",
        expires_at: "2026-10-05T12:15:00Z",
      },
      error: null,
    });
    const user = userEvent.setup();
    render(<LinearIntegrationSettings repoPath="/repo" />);

    await user.click(screen.getByRole("button", { name: /Connect via OAuth/ }));

    expect(invoke).toHaveBeenCalledWith("create-linear-oauth-intent", {
      body: {},
    });
    await waitFor(() =>
      expect(openUrl).toHaveBeenCalledWith(
        "https://linear.app/oauth/authorize?state=abc",
      ),
    );
    expect(useToastStore.getState().toasts).toEqual([]);
  });
});
