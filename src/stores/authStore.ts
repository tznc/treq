import type { Session, User } from "@supabase/supabase-js";
import { create } from "zustand";
import {
  cloudAccount,
  cloudExchangeToken,
  cloudRefreshSubscription,
  cloudSignOut,
  getSetting,
  setSetting,
} from "../lib/api";
import { remoteCutOffManaged } from "../lib/api-extra";
import { onWindowFocus } from "../lib/focus-refresh";
import {
  type RustAccount,
  type RustCloudWorkspace,
  listenRustAccount,
  rustOwnsAccount,
} from "../lib/rust-account";
import {
  SUPABASE_ANON_KEY,
  SUPABASE_URL,
  WEB_URL,
  sendAccessTokenFrom,
  supabase,
} from "../lib/supabase";
import { openUrl } from "@tauri-apps/plugin-opener";

export interface Subscription {
  /** `canceled` means set to cancel at the period end, still Pro until then. */
  status: "trialing" | "active" | "canceled" | "past_due" | "inactive";
  plan: "free" | "pro";
  current_period_end: string | null;
}

export interface AuthState {
  user: User | null;
  session: Session | null;
  loading: boolean;
  /** True while a sign-in token from a `treq://auth/callback` link is exchanged. */
  signingIn: boolean;
  /** Why the last token exchange failed; cleared when a new one starts. */
  signInError: string | null;
  subscription: Subscription | null;
  /** Rust's cloud workspace, when Rust owns the account; else null. */
  cloudWorkspace: RustCloudWorkspace | null;
  signIn: () => Promise<void>;
  signOut: () => Promise<void>;
  refreshSubscription: () => Promise<void>;
  exchangeToken: (token: string) => Promise<void>;
  restoreSession: () => Promise<void>;
  fetchSubscription: () => Promise<void>;
}

export const defaultAuthState = {
  user: null as User | null,
  session: null as Session | null,
  loading: true,
  signingIn: false,
  signInError: null as string | null,
  subscription: null as Subscription | null,
  cloudWorkspace: null as RustCloudWorkspace | null,
};

async function persistSession(sess: Session | null) {
  if (sess) {
    await setSetting(
      "supabase_session",
      JSON.stringify({
        accessToken: sess.access_token,
        refreshToken: sess.refresh_token,
      }),
    );
  } else {
    await setSetting("supabase_session", "");
  }
}

let authWatch: { unsubscribe: () => void } | null = null;

/**
 * Keeps the stored tokens in step with supabase-js. Refresh tokens rotate on
 * every refresh, and the old one stops working shortly after. Restoring a
 * session with an expired access token refreshes it, so without this the
 * next launch would present a dead refresh token and sign the user out.
 * Registered before the first setSession so that refresh is captured too.
 */
function watchAuthChanges() {
  if (authWatch) return;
  authWatch = supabase.auth.onAuthStateChange((event, session) => {
    if ((event === "TOKEN_REFRESHED" || event === "SIGNED_IN") && session) {
      useAuthStore.setState({ session, user: session.user });
      void persistSession(session).catch(() => undefined);
    } else if (event === "SIGNED_OUT") {
      useAuthStore.setState({ session: null, user: null, subscription: null });
      void persistSession(null).catch(() => undefined);
    }
  }).data.subscription;
}

/** The mirrored account's session, in supabase-js's shape. */
function sessionFrom(account: RustAccount): Session | null {
  const mirrored = account.auth.session;
  if (!mirrored) return null;
  const user = {
    id: mirrored.user.id,
    email: mirrored.user.email ?? undefined,
    user_metadata: mirrored.user.user_metadata ?? {},
    app_metadata: {},
    aud: "authenticated",
    created_at: "",
  } as User;
  return {
    access_token: mirrored.accessToken,
    refresh_token: mirrored.refreshToken,
    expires_at: mirrored.expiresAt,
    expires_in: Math.max(0, mirrored.expiresAt - Math.floor(Date.now() / 1000)),
    token_type: "bearer",
    user,
  };
}

function applyRustAccount(account: RustAccount) {
  const session = sessionFrom(account);
  useAuthStore.setState({
    session,
    user: session?.user ?? null,
    loading: account.auth.loading,
    signingIn: account.auth.signingIn,
    signInError: account.auth.signInError,
    subscription: account.subscription,
    cloudWorkspace: account.cloud,
  });
}

let mirroring: Promise<boolean> | null = null;

/**
 * Mirrors Rust's account (`rust-account.ts`): reads it, then follows its
 * `supabase-account` events, and sends its access token with supabase-js's
 * requests. False when Rust does not own it in this window.
 */
function mirrorRustAccount(): Promise<boolean> {
  mirroring ??= (async () => {
    const unlisten = await listenRustAccount(applyRustAccount);
    const account = await cloudAccount();
    if (!account) {
      unlisten();
      return false;
    }
    sendAccessTokenFrom(
      () => useAuthStore.getState().session?.access_token ?? null,
    );
    applyRustAccount(account);
    return true;
  })().catch(() => false);
  return mirroring;
}

/**
 * Refetches the subscription whenever the window comes back to the
 * foreground, so a purchase or cancellation made on the web dashboard shows
 * up without a restart. Returns a function that stops it. Rust rereads its
 * own on focus.
 */
export function startSubscriptionRefreshOnFocus(): () => void {
  if (rustOwnsAccount()) return () => undefined;
  return onWindowFocus(() => {
    void useAuthStore.getState().refreshSubscription();
  });
}

type SetAuth = (partial: Partial<AuthState>) => void;
type GetAuth = () => AuthState;

/** Trades a one-time desktop sign-in token for a Supabase session. */
async function exchangeTokenForSession(
  token: string,
  set: SetAuth,
  get: GetAuth,
) {
  const response = await fetch(
    `${SUPABASE_URL}/functions/v1/exchange-desktop-token`,
    {
      method: "POST",
      headers: {
        "Content-Type": "application/json",
        Authorization: `Bearer ${SUPABASE_ANON_KEY}`,
        apikey: SUPABASE_ANON_KEY,
      },
      body: JSON.stringify({ token }),
    },
  );

  if (!response.ok) {
    const err = await response.json().catch(() => ({}));
    throw new Error(
      (err as { error?: string }).error || "Token exchange failed",
    );
  }

  const data = await response.json();
  watchAuthChanges();
  const { data: sessionData, error } = await supabase.auth.setSession({
    access_token: data.access_token as string,
    refresh_token: data.refresh_token as string,
  });

  if (error) throw error;
  if (sessionData.session) {
    set({
      session: sessionData.session,
      user: sessionData.session.user,
    });
    await persistSession(sessionData.session);
    await get().fetchSubscription();
  }
}

export const useAuthStore = create<AuthState>((set, get) => ({
  ...defaultAuthState,
  signIn: async () => {
    await openUrl(`${WEB_URL}/sign-in?source=desktop`);
  },
  signOut: async () => {
    // Rust cuts managed access off, ends the session and clears its copy.
    if (rustOwnsAccount() && (await mirrorRustAccount())) {
      await cloudSignOut();
      return;
    }
    // Signing out ends the authorization behind managed SSH access, so
    // open managed connections and PTYs are cut off, not left running.
    try {
      await remoteCutOffManaged();
    } catch {
      // No native pool (e.g. a web preview); nothing to cut off.
    }
    await supabase.auth.signOut();
    set({ user: null, session: null, subscription: null });
    await persistSession(null);
  },
  fetchSubscription: async () => {
    if (rustOwnsAccount() && (await mirrorRustAccount())) {
      await cloudRefreshSubscription();
      return;
    }
    const { session } = get();
    if (!session) {
      set({ subscription: null });
      return;
    }
    try {
      const { data, error } = await supabase
        .from("subscriptions")
        .select("*")
        .single();
      if (!error && data) {
        set({ subscription: data as Subscription });
      }
    } catch {
      // Subscription fetch failed silently
    }
  },
  refreshSubscription: async () => {
    await get().fetchSubscription();
  },
  exchangeToken: async (token) => {
    if (rustOwnsAccount() && (await mirrorRustAccount())) {
      // Rust reports signing in and its error in the account it sends.
      await cloudExchangeToken(token);
      return;
    }
    set({ signingIn: true, signInError: null });
    try {
      await exchangeTokenForSession(token, set, get);
    } catch (err) {
      set({
        signInError:
          err instanceof Error ? err.message : "Token exchange failed",
      });
      throw err;
    } finally {
      set({ signingIn: false });
    }
  },
  restoreSession: async () => {
    if (rustOwnsAccount() && (await mirrorRustAccount())) return;
    try {
      const stored = await getSetting("supabase_session");
      if (stored) {
        const tokens = JSON.parse(stored) as Record<string, string>;
        watchAuthChanges();
        const { data, error } = await supabase.auth.setSession({
          access_token: tokens.accessToken,
          refresh_token: tokens.refreshToken,
        });
        if (!error && data.session) {
          set({
            session: data.session,
            user: data.session.user,
          });
          await persistSession(data.session);
          await get().fetchSubscription();
        }
      }
    } catch {
      // Failed to restore session
    } finally {
      set({ loading: false });
    }
  },
}));
