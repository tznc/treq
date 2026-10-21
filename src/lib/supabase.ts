import { createClient } from "@supabase/supabase-js";
import pkg from "../../package.json";

const environment = pkg.env[import.meta.env.PROD ? "prod" : "dev"];
const SUPABASE_URL = environment.supabase.url;
const SUPABASE_ANON_KEY = environment.supabase.anonKey;

export const supabase = createClient(SUPABASE_URL, SUPABASE_ANON_KEY, {
  auth: {
    autoRefreshToken: false,
    persistSession: false, // Desktop manages its own persistence via SQLite
  },
});

/**
 * Sends `read()`'s token with every request instead of supabase-js's own
 * session, which stays empty: the session is Rust's (`rust-account.ts`), so
 * supabase-js never refreshes or rotates it. A null token sends the anon key.
 */
export function sendAccessTokenFrom(read: () => string | null): void {
  // `SupabaseClient` reads its `accessToken` option on each request.
  (
    supabase as unknown as { accessToken?: () => Promise<string | null> }
  ).accessToken = async () => read();
}

const WEB_URL = environment.webUrl;

export { SUPABASE_URL, SUPABASE_ANON_KEY, WEB_URL };
