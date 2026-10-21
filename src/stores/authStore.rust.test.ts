import { invoke } from "@tauri-apps/api/core";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import type { RustAccount } from "../lib/rust-account";

const mocks = vi.hoisted(() => ({
  handlers: new Map<string, (event: { payload: unknown }) => void>(),
  auth: {
    setSession: vi.fn(),
    signOut: vi.fn(),
    onAuthStateChange: vi.fn(),
  },
  from: vi.fn(),
  sendAccessTokenFrom: vi.fn(),
}));

vi.mock("@tauri-apps/api/event", () => ({
  listen: vi.fn(
    async (name: string, handler: (event: { payload: unknown }) => void) => {
      mocks.handlers.set(name, handler);
      return () => mocks.handlers.delete(name);
    },
  ),
  emit: vi.fn(),
}));
vi.mock("../lib/supabase", () => ({
  supabase: { auth: mocks.auth, from: mocks.from },
  sendAccessTokenFrom: mocks.sendAccessTokenFrom,
  SUPABASE_URL: "http://127.0.0.1:54321",
  SUPABASE_ANON_KEY: "anon",
  WEB_URL: "http://localhost:3001",
}));

function account(patch: Partial<RustAccount["auth"]> = {}): RustAccount {
  return {
    auth: {
      loading: false,
      signingIn: false,
      signInError: null,
      session: {
        accessToken: "a1",
        refreshToken: "r1",
        expiresAt: 4_000_000_000,
        user: {
          id: "u1",
          email: "ada@example.com",
          user_metadata: { full_name: "Ada" },
        },
      },
      ...patch,
    },
    subscription: { status: "active", plan: "pro", current_period_end: null },
    cloud: {
      instanceStatus: null,
      provisioningStage: "Waiting",
      provisioningError: null,
      usage: null,
      usageError: null,
      submitting: true,
    },
  };
}

async function loadStore() {
  vi.resetModules();
  return (await import("./authStore")).useAuthStore;
}

beforeEach(() => {
  window.__TREQ_GPUI_SHELL__ = true;
  window.__TREQ_REACT_SUPABASE__ = false;
  mocks.handlers.clear();
  vi.mocked(invoke).mockReset();
  mocks.sendAccessTokenFrom.mockReset();
});

afterEach(() => {
  window.__TREQ_GPUI_SHELL__ = undefined;
  window.__TREQ_REACT_SUPABASE__ = undefined;
});

describe("authStore mirroring Rust's account", () => {
  it("restores the session Rust holds and sends its token", async () => {
    vi.mocked(invoke).mockImplementation(async (command) =>
      command === "cloud_account" ? account() : undefined,
    );
    const store = await loadStore();
    await store.getState().restoreSession();

    const state = store.getState();
    expect(state.loading).toBe(false);
    expect(state.user?.email).toBe("ada@example.com");
    expect(state.user?.user_metadata).toEqual({ full_name: "Ada" });
    expect(state.session?.access_token).toBe("a1");
    expect(state.subscription?.plan).toBe("pro");
    expect(state.cloudWorkspace?.provisioningStage).toBe("Waiting");
    // supabase-js never refreshes the session: Rust does.
    expect(mocks.auth.setSession).not.toHaveBeenCalled();
    const read = mocks.sendAccessTokenFrom.mock.calls[0]?.[0] as () => string;
    expect(read()).toBe("a1");
  });

  it("follows the account Rust sends", async () => {
    vi.mocked(invoke).mockImplementation(async (command) =>
      command === "cloud_account" ? account({ session: null }) : undefined,
    );
    const store = await loadStore();
    await store.getState().restoreSession();
    expect(store.getState().user).toBeNull();

    mocks.handlers.get("supabase-account")?.({ payload: account() });
    expect(store.getState().session?.access_token).toBe("a1");
    mocks.handlers.get("supabase-account")?.({
      payload: account({ session: null, signInError: "Token exchange failed" }),
    });
    expect(store.getState().session).toBeNull();
    expect(store.getState().signInError).toBe("Token exchange failed");
  });

  it("signs out, trades tokens and rereads the plan through Rust", async () => {
    vi.mocked(invoke).mockImplementation(async (command) =>
      command === "cloud_account" ? account() : undefined,
    );
    const store = await loadStore();
    await store.getState().restoreSession();
    await store.getState().exchangeToken("t1");
    await store.getState().refreshSubscription();
    await store.getState().signOut();

    const commands = vi
      .mocked(invoke)
      .mock.calls.map(([command, args]) => [command, args]);
    expect(commands).toEqual([
      ["cloud_account", undefined],
      ["cloud_exchange_token", { token: "t1" }],
      ["cloud_refresh_subscription", undefined],
      ["cloud_sign_out", undefined],
    ]);
    expect(mocks.auth.signOut).not.toHaveBeenCalled();
  });

  it("keeps React's own account where Rust has none", async () => {
    vi.mocked(invoke).mockResolvedValue(null);
    const store = await loadStore();
    await store.getState().restoreSession();
    expect(store.getState().loading).toBe(false);
    expect(mocks.sendAccessTokenFrom).not.toHaveBeenCalled();
    // React restores its own stored session.
    expect(vi.mocked(invoke).mock.calls.map(([c]) => c)).toContain(
      "get_setting",
    );
  });
});
