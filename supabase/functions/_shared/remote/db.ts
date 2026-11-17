// One error check for every Supabase query and RPC in the remote and billing
// modules: return the data, or throw with what we were doing.

import type { SupabaseClient } from "npm:@supabase/supabase-js@2.95.3";

/** Returns `res.data`, or throws "failed to <what>: <message>". */
export function must<T>(res: { data: T; error: { message: string } | null }, what: string): T {
  if (res.error) throw new Error(`failed to ${what}: ${res.error.message}`);
  return res.data;
}

/** Calls an RPC and returns its data, or throws "<fn> failed: <message>". */
export async function rpc<T = unknown>(
  supabase: SupabaseClient,
  fn: string,
  args?: Record<string, unknown>,
): Promise<T> {
  const { data, error } = await supabase.rpc(fn, args);
  if (error) throw new Error(`${fn} failed: ${error.message}`);
  return data as T;
}
