import { useEffect } from "react";
import { ensureLinearProxySessionSync } from "../lib/linear-proxy-auth";

/**
 * Starts pushing the Supabase session to the Rust Linear client once the
 * Linear integration is on, so OAuth-connected repos can reach Linear
 * through the proxy.
 */
export function useLinearProxySession(enabled: boolean) {
  useEffect(() => {
    if (!enabled) return;
    ensureLinearProxySessionSync().catch((err) => {
      console.warn("Failed to sync the Linear proxy session", err);
    });
  }, [enabled]);
}
