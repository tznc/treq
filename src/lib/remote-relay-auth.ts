// Keeps the native SSH pool supplied with the Supabase access token it
// sends to the `remote-ssh-relay` Edge Function. Managed Sprites have no raw
// TCP ingress, so every managed SSH connection opens a WebSocket to that
// relay and authenticates with the user's session token in a header.
//
// The Rust side reads the token when it opens a connection, so this only
// has to keep the latest token there. See `supabase-token-sync.ts` for how
// the token stays fresh.

import { remoteSetRelayAccessToken } from "./api-extra";
import { rustOwnsAccount } from "./rust-account";
import {
  type AccessTokenSyncDeps,
  startAccessTokenSync,
  supabaseTokenDeps,
} from "./supabase-token-sync";

export type RelayAuthDeps = AccessTokenSyncDeps;

let started: Promise<void> | null = null;

/**
 * Starts pushing the session token to the native pool, once per app run.
 * Resolves after the first push so the caller's first relayed connection
 * already has a token. Safe to call on every managed connect.
 */
export function ensureRelayAccessTokenSync(
  deps: RelayAuthDeps = supabaseTokenDeps(remoteSetRelayAccessToken),
): Promise<void> {
  if (started) return started;
  // Rust pushes its own session's token on each refresh.
  started = rustOwnsAccount() ? Promise.resolve() : startAccessTokenSync(deps);
  return started;
}

/** Test-only: forget that sync was started. */
export function resetRelayAccessTokenSyncForTests(): void {
  started = null;
}
