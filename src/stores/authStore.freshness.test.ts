import { beforeEach, describe, expect, it, vi } from "vitest";

type AuthListener = (event: string, session: unknown) => void;

const mocks = vi.hoisted(() => {
  const listeners: Array<(event: string, session: unknown) => void> = [];
  const subscriptionQuery = {
    select: vi.fn(),
    single: vi.fn(),
  };
  subscriptionQuery.select.mockReturnValue(subscriptionQuery);
  return {
    listeners,
    subscriptionQuery,
    auth: {
      setSession: vi.fn(),
      signOut: vi.fn(),
      onAuthStateChange: vi.fn((listener: (e: string, s: unknown) => void) => {
        listeners.push(listener);
        return { data: { subscription: { unsubscribe: vi.fn() } } };
      }),
    },
    from: vi.fn(() => subscriptionQuery),
    getSetting: vi.fn(),
    setSetting: vi.fn(),
  };
});

vi.mock("../lib/supabase", () => ({
  supabase: { auth: mocks.auth, from: mocks.from },
  SUPABASE_URL: "http://127.0.0.1:54321",
  SUPABASE_ANON_KEY: "anon",
  WEB_URL: "http://localhost:3001",
}));
vi.mock("../lib/api", () => ({
  getSetting: mocks.getSetting,
  setSetting: mocks.setSetting,
}));
vi.mock("../lib/api-extra", () => ({ remoteCutOffManaged: vi.fn() }));

function session(access: string, refresh: string) {
  return {
    access_token: access,
    refresh_token: refresh,
    user: { id: "user-1", email: "dev@example.com" },
  };
}

function storedTokens(): Array<string> {
  return mocks.setSetting.mock.calls
    .filter(([key]) => key === "supabase_session")
    .map(([, value]) => value as string);
}

async function loadStore() {
  vi.resetModules();
  return (await import("./authStore")).useAuthStore;
}

function emit(event: string, value: unknown) {
  for (const listener of mocks.listeners as AuthListener[]) {
    listener(event, value);
  }
}

beforeEach(() => {
  mocks.listeners.length = 0;
  mocks.auth.onAuthStateChange.mockClear();
  mocks.auth.setSession.mockReset();
  mocks.setSetting.mockReset().mockResolvedValue(undefined);
  mocks.getSetting.mockReset().mockResolvedValue(
    JSON.stringify({
      accessToken: "old-access",
      refreshToken: "old-refresh",
    }),
  );
  mocks.subscriptionQuery.single.mockReset().mockResolvedValue({
    data: { plan: "free", status: "inactive", current_period_end: null },
    error: null,
  });
});

describe("refreshed Supabase tokens", () => {
  it("persists the tokens supabase-js rotates while restoring a session", async () => {
    const rotated = session("new-access", "new-refresh");
    mocks.auth.setSession.mockImplementation(async () => {
      // setSession refreshes an expired access token, rotating the refresh token.
      emit("TOKEN_REFRESHED", rotated);
      emit("SIGNED_IN", rotated);
      return { data: { session: rotated }, error: null };
    });
    const useAuthStore = await loadStore();

    await useAuthStore.getState().restoreSession();

    expect(storedTokens()).toContain(
      JSON.stringify({
        accessToken: "new-access",
        refreshToken: "new-refresh",
      }),
    );
    expect(storedTokens()).not.toContain(
      JSON.stringify({
        accessToken: "old-access",
        refreshToken: "old-refresh",
      }),
    );
  });

  it("persists a refresh that happens after sign-in", async () => {
    const restored = session("old-access", "old-refresh");
    mocks.auth.setSession.mockResolvedValue({
      data: { session: restored },
      error: null,
    });
    const useAuthStore = await loadStore();
    await useAuthStore.getState().restoreSession();

    const later = session("later-access", "later-refresh");
    emit("TOKEN_REFRESHED", later);

    await vi.waitFor(() =>
      expect(storedTokens().at(-1)).toBe(
        JSON.stringify({
          accessToken: "later-access",
          refreshToken: "later-refresh",
        }),
      ),
    );
    expect(useAuthStore.getState().session).toBe(later);
  });

  it("forgets the stored session when supabase-js signs the user out", async () => {
    mocks.auth.setSession.mockResolvedValue({
      data: { session: session("a", "r") },
      error: null,
    });
    const useAuthStore = await loadStore();
    await useAuthStore.getState().restoreSession();

    emit("SIGNED_OUT", null);

    await vi.waitFor(() => expect(storedTokens().at(-1)).toBe(""));
    expect(useAuthStore.getState().user).toBeNull();
    expect(useAuthStore.getState().subscription).toBeNull();
  });

  it("listens for auth changes once however often the session is restored", async () => {
    mocks.auth.setSession.mockResolvedValue({
      data: { session: session("a", "r") },
      error: null,
    });
    const useAuthStore = await loadStore();
    await useAuthStore.getState().restoreSession();
    await useAuthStore.getState().restoreSession();

    expect(mocks.auth.onAuthStateChange).toHaveBeenCalledTimes(1);
  });
});

describe("subscription refresh on window focus", () => {
  it("refetches the subscription when the window regains focus", async () => {
    mocks.auth.setSession.mockResolvedValue({
      data: { session: session("a", "r") },
      error: null,
    });
    const useAuthStore = await loadStore();
    const { startSubscriptionRefreshOnFocus } = await import("./authStore");
    await useAuthStore.getState().restoreSession();
    expect(useAuthStore.getState().subscription?.plan).toBe("free");

    // The user finished checkout in the browser and came back.
    mocks.subscriptionQuery.single.mockResolvedValue({
      data: {
        plan: "pro",
        status: "trialing",
        current_period_end: "2026-10-19T12:00:00",
      },
      error: null,
    });
    vi.useFakeTimers();
    const stop = startSubscriptionRefreshOnFocus();
    try {
      window.dispatchEvent(new Event("focus"));
      window.dispatchEvent(new Event("focus"));
      await vi.runAllTimersAsync();
    } finally {
      stop();
      vi.useRealTimers();
    }

    await vi.waitFor(() =>
      expect(useAuthStore.getState().subscription).toEqual({
        plan: "pro",
        status: "trialing",
        current_period_end: "2026-10-19T12:00:00",
      }),
    );
    // One fetch on restore, one for the burst of focus events.
    expect(mocks.subscriptionQuery.single).toHaveBeenCalledTimes(2);
  });
});
