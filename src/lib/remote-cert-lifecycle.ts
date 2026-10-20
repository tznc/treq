// Certificate auto-renewal and hard cutoff for managed-VM SSH certificates
// (prds/remote-development.md, "SSH identity and certificates" > "Silent renewal
// while the session is active" and "Hard cutoff on revocation or expiry").
//
// A certificate must never lapse under a user who remains authenticated.
// `CertificateRenewalManager` requests a fresh certificate ahead of expiry,
// transparently - it never interrupts open exec/PTY channels (those live in
// the Rust `SshConnectionPool` and only get torn down on an actual cutoff)
// and never prompts the user. Renewal is refused, and the certificate is
// allowed to lapse, only when the underlying authorization is no longer
// valid: the Supabase session has ended, the client's key has been revoked,
// or the instance is no longer accessible to that user - see
// `classifyRenewalError`. In every one of those cases (plus a plain
// unrenewed expiry) the manager calls `onCutoff`, whose real wiring
// (`startManagedCertificateRenewal`) forces the native transport's hard
// cutoff via the `remote_force_cutoff` Tauri command.

import type { IssueCertificateResponse } from "./api-types-remote";
import { remoteForceCutoff } from "./api-extra";
import { useAuthStore } from "../stores/authStore";
import { issueCertificate, reportCutoff } from "./remote-control-plane";
import { rustOwnsAccount } from "./rust-account";
import { supabase } from "./supabase";

/**
 * Renew once at most this fraction of the certificate's total lifetime
 * remains (20%: a 20-minute certificate - see
 * `CERTIFICATE_VALIDITY_MINUTES` in `supabase/functions/remote-ssh-trust` -
 * is renewed with roughly 4 minutes left). This mirrors the way an OAuth 2
 * access token is refreshed ahead of expiry (PRD: "the same way an OAuth 2
 * access token is refreshed ahead of expiry"): renewing well before expiry
 * absorbs normal request latency and transient network failures without
 * ever letting a live session go uncertified, while still keeping the
 * window an unrenewable (e.g. stolen) certificate remains usable short.
 */
export const RENEWAL_REMAINING_LIFETIME_FRACTION = 0.2;

const RETRY_BACKOFF_BASE_MS = 15_000;
const MAX_RETRY_BACKOFF_MS = 2 * 60_000;

export type CutoffReason =
  | "session_ended"
  | "key_revoked"
  | "instance_inaccessible"
  | "certificate_expired";

export interface CertificateLease {
  instanceId: string;
  keyId: string;
  endpointId: string;
  serial: string;
  /**
   * The issued OpenSSH certificate text. Renewal hands the fresh one to the
   * caller so it can update the active endpoint; only connections opened
   * after that pick it up, open channels keep running.
   */
  certificate: string;
  /** Epoch ms this lease was obtained (approximated client-side as "now"). */
  issuedAt: number;
  /** Epoch ms the certificate stops being valid. */
  expiresAt: number;
}

/**
 * How long to wait, from `now`, before the next renewal attempt for a
 * certificate valid from `issuedAt` to `expiresAt`. Renews once
 * `RENEWAL_REMAINING_LIFETIME_FRACTION` of the lifetime remains; a
 * certificate already past that point (e.g. after a slow resume from
 * sleep) is renewed immediately (delay 0), never late.
 */
export function renewalDelayMs(
  issuedAt: number,
  expiresAt: number,
  now: number,
): number {
  const lifetime = expiresAt - issuedAt;
  if (lifetime <= 0) return 0;
  const renewAt =
    issuedAt + lifetime * (1 - RENEWAL_REMAINING_LIFETIME_FRACTION);
  return Math.max(0, renewAt - now);
}

interface FunctionsErrorLike {
  status?: number;
  context?: { status?: number };
  message?: string;
}

/** Reads the HTTP status from a `RemoteFunctionError` (what the
 * control-plane client throws) or a raw supabase-js `FunctionsHttpError`. */
function statusOf(error: unknown): number | undefined {
  const err = error as FunctionsErrorLike;
  if (typeof err?.status === "number") return err.status;
  return typeof err?.context?.status === "number"
    ? err.context.status
    : undefined;
}

function messageOf(error: unknown): string {
  const err = error as FunctionsErrorLike;
  return typeof err?.message === "string" ? err.message.toLowerCase() : "";
}

/**
 * Classifies a failed renewal call. `"retry"` means the failure looks
 * transient (network error, 5xx, rate limit) and the manager should back off
 * and try again before the certificate actually expires. Any `CutoffReason`
 * means the PRD's "renewal is refused... only when the underlying
 * authorization is no longer valid" condition was hit and the manager must
 * not retry - it proceeds straight to cutoff.
 */
export function classifyRenewalError(error: unknown): "retry" | CutoffReason {
  const status = statusOf(error);
  if (status === 401) return "session_ended";
  if (status === 404) {
    // "Key does not belong to this user": the key row is gone.
    return messageOf(error).includes("key does not belong")
      ? "key_revoked"
      : "instance_inaccessible";
  }
  if (status === 409) {
    return messageOf(error).includes("revoked")
      ? "key_revoked"
      : "instance_inaccessible";
  }
  return "retry";
}

export interface CertificateRenewalManagerOptions {
  instanceId: string;
  keyId: string;
  endpointId: string;
  initialLease: { issuedAt: number; expiresAt: number };
  /** Returns whether the Supabase session is currently valid. */
  isSessionValid: () => boolean | Promise<boolean>;
  /** Requests a fresh certificate; swappable for tests. */
  issue: (
    instanceId: string,
    keyId: string,
  ) => Promise<IssueCertificateResponse>;
  onRenewed: (lease: CertificateLease) => void;
  onCutoff: (reason: CutoffReason) => void;
  now?: () => number;
}

/**
 * Schedules silent certificate renewal for one managed-instance endpoint.
 * Never interrupts open channels itself - it only ever requests a new
 * certificate and, on an unrecoverable failure, hands off to `onCutoff` so
 * the caller can force the transport-level hard cutoff.
 */
export class CertificateRenewalManager {
  private timer: ReturnType<typeof setTimeout> | null = null;
  private stopped = false;
  private retryCount = 0;
  private lease: { issuedAt: number; expiresAt: number };
  private readonly opts: CertificateRenewalManagerOptions;

  constructor(opts: CertificateRenewalManagerOptions) {
    this.opts = opts;
    this.lease = opts.initialLease;
    this.scheduleRenewal();
  }

  private now(): number {
    return (this.opts.now ?? Date.now)();
  }

  private scheduleRenewal(): void {
    if (this.stopped) return;
    if (this.timer) clearTimeout(this.timer);
    const delay = renewalDelayMs(
      this.lease.issuedAt,
      this.lease.expiresAt,
      this.now(),
    );
    this.timer = setTimeout(() => {
      void this.attemptRenewal();
    }, delay);
  }

  private scheduleRetry(): void {
    if (this.stopped) return;
    const backoff = Math.min(
      MAX_RETRY_BACKOFF_MS,
      RETRY_BACKOFF_BASE_MS * 2 ** this.retryCount,
    );
    this.retryCount += 1;
    const timeLeft = this.lease.expiresAt - this.now();
    if (timeLeft <= backoff) {
      // Not enough time left to try again before the certificate actually
      // expires - let it lapse per the hard-cutoff path rather than
      // spuriously retrying past expiry.
      if (this.timer) clearTimeout(this.timer);
      this.timer = setTimeout(
        () => this.cutoff("certificate_expired"),
        Math.max(0, timeLeft),
      );
      return;
    }
    if (this.timer) clearTimeout(this.timer);
    this.timer = setTimeout(() => {
      void this.attemptRenewal();
    }, backoff);
  }

  private cutoff(reason: CutoffReason): void {
    if (this.stopped) return;
    this.stopped = true;
    if (this.timer) clearTimeout(this.timer);
    this.opts.onCutoff(reason);
  }

  private async attemptRenewal(): Promise<void> {
    if (this.stopped) return;
    let sessionValid: boolean;
    try {
      sessionValid = await this.opts.isSessionValid();
    } catch {
      sessionValid = false;
    }
    if (!sessionValid) {
      this.cutoff("session_ended");
      return;
    }
    try {
      const response = await this.opts.issue(
        this.opts.instanceId,
        this.opts.keyId,
      );
      this.retryCount = 0;
      const issuedAt = this.now();
      const expiresAt = Date.parse(response.expires_at);
      this.lease = { issuedAt, expiresAt };
      this.opts.onRenewed({
        instanceId: this.opts.instanceId,
        keyId: this.opts.keyId,
        endpointId: this.opts.endpointId,
        serial: response.serial,
        certificate: response.certificate,
        issuedAt,
        expiresAt,
      });
      this.scheduleRenewal();
    } catch (error) {
      const outcome = classifyRenewalError(error);
      if (outcome === "retry") {
        this.scheduleRetry();
        return;
      }
      this.cutoff(outcome);
    }
  }

  /** Stops scheduling further renewals without triggering a cutoff. */
  stop(): void {
    this.stopped = true;
    if (this.timer) clearTimeout(this.timer);
  }
}

/**
 * Real wiring: renews through the `remote-ssh-trust` edge function, checks
 * Supabase session validity through the local `supabase-js` client, and on
 * cutoff forces the native transport's hard cutoff for `endpointId` via the
 * `remote_force_cutoff` Tauri command (see
 * `src-tauri/src/commands/remote_control.rs`) so open exec/PTY channels are
 * torn down and the UI can react to the `remote://cutoff` event.
 */
export function startManagedCertificateRenewal(
  lease: CertificateLease,
  onRenewed?: (lease: CertificateLease) => void,
): CertificateRenewalManager {
  return new CertificateRenewalManager({
    instanceId: lease.instanceId,
    keyId: lease.keyId,
    endpointId: lease.endpointId,
    initialLease: { issuedAt: lease.issuedAt, expiresAt: lease.expiresAt },
    isSessionValid: async () => {
      // Rust's session, mirrored, when it owns the account; it refreshes
      // the session itself.
      const session = rustOwnsAccount()
        ? useAuthStore.getState().session
        : await supabase.auth
            .getSession()
            .then(({ data, error }) => (error ? null : data.session));
      if (!session) return false;
      const expiresAtMs = (session.expires_at ?? 0) * 1000;
      return expiresAtMs === 0 || expiresAtMs > Date.now();
    },
    issue: async (instanceId, keyId) =>
      issueCertificate({
        instance_id: instanceId,
        key_id: keyId,
        // Distinguishes a renewal from first issuance in the server-side
        // audit trail only (see `remote-ssh-trust`'s `handleIssueCertificate`
        // and its `isRenewal` flag) - the signing logic is identical either
        // way.
        renewal: true,
      }),
    onRenewed: (renewed) => onRenewed?.(renewed),
    onCutoff: (reason) => {
      void remoteForceCutoff(lease.endpointId, reason);
      void reportCutoff({
        instance_id: lease.instanceId,
        endpoint_id: lease.endpointId,
        reason,
      }).catch(() => {
        // Best-effort audit report; the local cutoff already took effect.
      });
    },
  });
}
