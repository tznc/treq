/**
 * Supabase clients for service-qa specs.
 *
 * Prefer keys from `supabase status -o env` so a non-default local project
 * still works. Fall back to the standard local demo JWTs (same anon key as
 * package.json → env.dev.supabase).
 */
import { createClient, type SupabaseClient } from "@supabase/supabase-js";
import { execFileSync } from "node:child_process";

export const LOCAL_SUPABASE_URL = "http://127.0.0.1:54321";

/** Standard local demo anon JWT (supabase start default). */
export const LOCAL_ANON_KEY =
  "eyJhbGciOiJIUzI1NiIsInR5cCI6IkpXVCJ9.eyJpc3MiOiJzdXBhYmFzZS1kZW1vIiwicm9sZSI6ImFub24iLCJleHAiOjE5ODM4MTI5OTZ9.CRXP1A7WOeoJeXxjNni43kdQwgnWNReilDMblYTn_I0";

/** Standard local demo service_role JWT (supabase start default). */
export const LOCAL_SERVICE_ROLE_KEY =
  "eyJhbGciOiJIUzI1NiIsInR5cCI6IkpXVCJ9.eyJpc3MiOiJzdXBhYmFzZS1kZW1vIiwicm9sZSI6InNlcnZpY2Vfcm9sZSIsImV4cCI6MTk4MzgxMjk5Nn0.EGIM96RAZx35lJzdJsyH-qQwv8Hdp7fsn3W0YpN81IU";

type LocalKeys = {
  url: string;
  anonKey: string;
  serviceRoleKey: string;
};

/** What `supabase status` printed: a key it left out is undefined. */
type StatusKeys = { [K in keyof LocalKeys]: string | undefined };

let cachedKeys: LocalKeys | null = null;

function parseStatusEnv(stdout: string): StatusKeys {
  const pick = (name: string): string | undefined => {
    const match = stdout.match(new RegExp(`^${name}=\"?([^\"]+)\"?$`, "m"));
    return match?.[1];
  };
  return {
    url: pick("API_URL") ?? pick("SUPABASE_URL"),
    anonKey: pick("ANON_KEY") ?? pick("SUPABASE_ANON_KEY"),
    serviceRoleKey:
      pick("SERVICE_ROLE_KEY") ?? pick("SUPABASE_SERVICE_ROLE_KEY"),
  };
}

export function resolveLocalKeys(): LocalKeys {
  if (cachedKeys) return cachedKeys;

  let fromStatus: Partial<StatusKeys> = {};
  try {
    const stdout = execFileSync("supabase", ["status", "-o", "env"], {
      encoding: "utf8",
      stdio: ["ignore", "pipe", "pipe"],
      timeout: 15_000,
    });
    fromStatus = parseStatusEnv(stdout);
  } catch {
    // CLI missing or stack down — fall back to demo keys; health.ts fails the run.
  }

  cachedKeys = {
    url: fromStatus.url ?? LOCAL_SUPABASE_URL,
    anonKey: fromStatus.anonKey ?? LOCAL_ANON_KEY,
    serviceRoleKey: fromStatus.serviceRoleKey ?? LOCAL_SERVICE_ROLE_KEY,
  };
  return cachedKeys;
}

export function getAnonClient(): SupabaseClient {
  const { url, anonKey } = resolveLocalKeys();
  return createClient(url, anonKey, {
    auth: { autoRefreshToken: false, persistSession: false },
  });
}

export function getServiceClient(): SupabaseClient {
  const { url, serviceRoleKey } = resolveLocalKeys();
  return createClient(url, serviceRoleKey, {
    auth: { autoRefreshToken: false, persistSession: false },
  });
}

export function getFunctionsBaseUrl(): string {
  return `${resolveLocalKeys().url}/functions/v1`;
}

export function getAnonKey(): string {
  return resolveLocalKeys().anonKey;
}
