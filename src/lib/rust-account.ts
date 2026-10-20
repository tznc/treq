// In the desktop app the Rust side owns the Supabase account: it restores
// and refreshes the session, signs out, trades the sign-in link's token,
// reads the plan and runs the cloud workspace (`src-tauri/src/core/cloud.rs`).
// The webview mirrors what it sends, so supabase-js never refreshes (and so
// rotates) the refresh token Rust holds. The commands are in `api.ts`.

import { listen, type UnlistenFn } from "@tauri-apps/api/event";
import type {
  InstanceStatusResponse,
  MachineUsageReport,
  SshEndpoint,
} from "./api-types-remote";
import { isGpuiShellHost } from "./gpui-shell";

declare global {
  interface Window {
    /** React owns the Supabase account, as without the GPUI shell; tests set it. */
    __TREQ_REACT_SUPABASE__?: boolean;
  }
}

/** True when the account is Rust's and the webview only mirrors it. */
export function rustOwnsAccount(): boolean {
  return isGpuiShellHost() && window.__TREQ_REACT_SUPABASE__ !== true;
}

interface RustAccountSession {
  accessToken: string;
  refreshToken: string;
  /** Unix seconds. */
  expiresAt: number;
  user: {
    id: string;
    email: string | null;
    user_metadata: Record<string, unknown> | null;
  };
}

/** `CloudWorkspace`: the cloud workspace's state, as Dashboard keeps it. */
export interface RustCloudWorkspace {
  instanceStatus: InstanceStatusResponse | null;
  provisioningStage: string | null;
  provisioningError: string | null;
  usage: MachineUsageReport | null;
  usageError: string | null;
  submitting: boolean;
}

/** `AccountSnapshot`: the session, the plan and the cloud workspace. */
export interface RustAccount {
  auth: {
    loading: boolean;
    signingIn: boolean;
    signInError: string | null;
    session: RustAccountSession | null;
  };
  subscription: {
    status: "trialing" | "active" | "canceled" | "past_due" | "inactive";
    plan: "free" | "pro";
    current_period_end: string | null;
  } | null;
  cloud: RustCloudWorkspace;
}

/** `CloudAction`: what the cloud workspace screens ask Rust to run. */
export type RustCloudAction =
  | {
      action:
        | "refresh"
        | "provision"
        | "wake"
        | "repair"
        | "delete"
        | "openRepositories";
    }
  | { action: "reauthenticate"; endpointId: string }
  | { action: "revokeKey"; keyReference: string };

/** `ManagedEndpointEvent`: Rust connected, or renewed, a managed endpoint. */
type ManagedEndpointEvent =
  | {
      kind: "connected";
      endpoint: SshEndpoint;
      generation: number;
      openRepositories: boolean;
    }
  | { kind: "renewed"; endpoint: SshEndpoint };

export function listenRustAccount(
  handler: (account: RustAccount) => void,
): Promise<UnlistenFn> {
  return listen<RustAccount>("supabase-account", (event) =>
    handler(event.payload),
  );
}

export function listenManagedEndpoint(
  handler: (event: ManagedEndpointEvent) => void,
): Promise<UnlistenFn> {
  return listen<ManagedEndpointEvent>("managed-endpoint", (event) =>
    handler(event.payload),
  );
}
