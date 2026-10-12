// Connection state for the mobile shell: one active endpoint, either the
// user's Treq-managed instance (certificate auth with the device key held in
// the OS keystore) or a user-managed SSH endpoint saved on this device.
//
// The managed path uses the same state machine as desktop
// (`lib/managed-ssh-connection.ts`): register the key, issue a certificate,
// activate the endpoint, and keep the certificate renewed in the background
// (`lib/remote-cert-lifecycle.ts`). Renewal failures that mean access has
// ended force the transport's hard cutoff, which `useRemoteCutoffStore`
// reports so the shell can block interaction until the user reauthenticates.

import { useEffect, useRef, useState } from "react";
import { ensureMobileDeviceKey } from "../lib/api";
import {
  DEVICE_KEYSTORE_KEY_REFERENCE,
  type ManagedInstanceState,
  type SshEndpoint,
} from "../lib/api-types-remote";
import {
  connectExistingReadyInstance,
  reauthenticateManagedInstance,
  waitForInstanceReady,
  wakeManagedInstance,
  type ManagedConnectionDeps,
  type ManagedConnectionResult,
  type RenewalController,
} from "../lib/managed-ssh-connection";
import {
  renewalDelayMs,
  startManagedCertificateRenewal,
  type CertificateLease,
} from "../lib/remote-cert-lifecycle";
import {
  getInstanceStatus,
  issueCertificate,
  registerClientKey,
  wakeInstance,
} from "../lib/remote-control-plane";
import { ensureRelayAccessTokenSync } from "../lib/remote-relay-auth";
import {
  sshEndpointFromUserManaged,
  type UserManagedEndpointRecord,
} from "../lib/remote-endpoints";
import type { MobileEndpointChoice } from "../lib/mobile-session";
import { useRemoteCutoffStore } from "../stores/remoteCutoffStore";

const DEVICE_KEY_COMMENT = "treq-mobile-device";

// Mirrors `SECURE_STORAGE_UNAVAILABLE_PREFIX` in
// `src-tauri/src/core/remote_device_key.rs`. `ensure_mobile_device_key`
// prefixes its error string this way when the device's secure key storage
// (Android Keystore / iOS Keychain, gated on biometrics) isn't usable, so
// this state can be told apart from a generic connection failure.
//
// Exported so `RemoteConnectPanel.test.tsx` can assert this literal stays
// byte-for-byte in sync with the Rust constant - see that test for why a
// plain string prefix, rather than a generated binding, is what needs
// guarding here.
export const SECURE_STORAGE_UNAVAILABLE_PREFIX = "secure_storage_unavailable:";

// Mobile does not provision (mobile PRD, "Non-goals": owning managed-VM
// provisioning inside the mobile UI), so an account without an instance is
// sent to desktop.
const NO_MANAGED_INSTANCE_MESSAGE =
  "No managed instance for this account yet. Set one up from Treq on desktop, then connect here.";

type MobileConnectionStatus =
  | "idle"
  | "connecting"
  | "connected"
  | "error"
  | "secure_storage_unavailable";

export interface MobileConnectionState {
  status: MobileConnectionStatus;
  /** What a connect attempt is doing right now, for the progress label. */
  step: string | null;
  endpoint: SshEndpoint | null;
  choice: MobileEndpointChoice | null;
  /** Last lifecycle state the control plane reported for the managed instance. */
  instanceState: ManagedInstanceState | null;
  error: string | null;
}

/** What a resume check did: nothing needed, a fresh connection, or a failure. */
export type ResumeOutcome = "unchanged" | "reconnected" | "failed";

const INITIAL_STATE: MobileConnectionState = {
  status: "idle",
  step: null,
  endpoint: null,
  choice: null,
  instanceState: null,
  error: null,
};

function errorMessage(err: unknown): string {
  return err instanceof Error ? err.message : String(err);
}

export function useMobileRemoteConnection() {
  const [state, setState] = useState<MobileConnectionState>(INITIAL_STATE);
  // Callers such as the resume handler run outside render, so they read the
  // latest state and lease from refs rather than from a stale closure.
  const stateRef = useRef(state);
  useEffect(() => {
    stateRef.current = state;
  });
  const leaseRef = useRef<CertificateLease | null>(null);
  const renewalRef = useRef<RenewalController | null>(null);

  const stopRenewal = () => {
    renewalRef.current?.stop();
    renewalRef.current = null;
  };
  useEffect(() => stopRenewal, []);

  const setStep = (step: string) => setState((s) => ({ ...s, step }));

  const fail = (err: unknown) => {
    const message = errorMessage(err);
    const secureStorage = message.startsWith(SECURE_STORAGE_UNAVAILABLE_PREFIX);
    setState((s) => ({
      ...s,
      status: secureStorage ? "secure_storage_unavailable" : "error",
      step: null,
      error: secureStorage
        ? message.slice(SECURE_STORAGE_UNAVAILABLE_PREFIX.length)
        : message,
    }));
  };

  const managedDeps = (): ManagedConnectionDeps => {
    let fingerprint: string | null = null;
    return {
      readPublicKey: async () => {
        const key = await ensureMobileDeviceKey();
        fingerprint = key.fingerprint_sha256;
        return key.public_key;
      },
      // Keyed on the device key's fingerprint rather than the shared
      // `keystore:device` reference, which is the same on every phone.
      registerClientKey: (publicKey) =>
        registerClientKey({
          public_key: publicKey,
          comment: DEVICE_KEY_COMMENT,
          idempotency_key: `register:${fingerprint ?? publicKey}`,
        }),
      ensureInstance: () =>
        Promise.reject(new Error(NO_MANAGED_INSTANCE_MESSAGE)),
      getInstanceStatus: async () => {
        const status = await getInstanceStatus();
        setState((s) => ({
          ...s,
          instanceState: status.instance?.status ?? null,
        }));
        return status;
      },
      issueCertificate: (instanceId, keyId) =>
        issueCertificate({ instance_id: instanceId, key_id: keyId }),
      activateEndpoint: (endpoint) => {
        stopRenewal();
        setState((s) => ({ ...s, endpoint }));
      },
      // A renewed certificate for the same endpoint; open channels keep
      // running and the next connection presents the new certificate.
      updateEndpointCertificate: (endpoint) =>
        setState((s) =>
          s.endpoint?.id === endpoint.id ? { ...s, endpoint } : s,
        ),
      startRenewal: (lease, onRenewed) => {
        leaseRef.current = lease;
        return startManagedCertificateRenewal(lease, (renewed) => {
          leaseRef.current = renewed;
          onRenewed?.(renewed);
        });
      },
      prepareRelay: () => ensureRelayAccessTokenSync(),
      clearCutoff: (endpointId) =>
        useRemoteCutoffStore.getState().clearCutoff(endpointId),
      wakeInstance: (instanceId, idempotencyKey) =>
        wakeInstance({
          instance_id: instanceId,
          idempotency_key: idempotencyKey,
        }),
    };
  };

  const adopt = (result: ManagedConnectionResult): SshEndpoint => {
    renewalRef.current = result.renewal;
    setState((s) => ({
      ...s,
      status: "connected",
      step: null,
      endpoint: result.endpoint,
      choice: { kind: "managed" },
      error: null,
    }));
    return result.endpoint;
  };

  /**
   * Connects to the managed instance from the control plane's current
   * status: wakes a suspended instance, waits for one that is waking, and
   * always issues a fresh certificate for the device key.
   */
  const connectManaged = async (): Promise<SshEndpoint | null> => {
    setState((s) => ({
      ...s,
      status: "connecting",
      step: "Checking the managed instance",
      choice: { kind: "managed" },
      error: null,
    }));
    const deps = managedDeps();
    try {
      const status = await deps.getInstanceStatus();
      const { instance } = status;
      if (!instance) throw new Error(NO_MANAGED_INSTANCE_MESSAGE);
      if (instance.status === "suspended") {
        setStep("Waking the managed instance");
        return adopt(
          await wakeManagedInstance(deps, {
            instanceId: instance.instance_id,
            keyReference: DEVICE_KEYSTORE_KEY_REFERENCE,
          }),
        );
      }
      let ready = status;
      if (instance.status === "waking") {
        setStep("Waiting for the managed instance");
        ready = await waitForInstanceReady(deps);
      }
      setStep("Getting a certificate");
      return adopt(
        await connectExistingReadyInstance(deps, {
          status: ready,
          keyReference: DEVICE_KEYSTORE_KEY_REFERENCE,
        }),
      );
    } catch (err) {
      fail(err);
      return null;
    }
  };

  const connectUserManaged = (
    record: UserManagedEndpointRecord,
  ): SshEndpoint => {
    stopRenewal();
    leaseRef.current = null;
    const endpoint = sshEndpointFromUserManaged(record);
    setState({
      ...INITIAL_STATE,
      status: "connected",
      endpoint,
      choice: { kind: "user_managed", id: record.id },
    });
    return endpoint;
  };

  /**
   * After a hard cutoff: registers the key again and asks for a new
   * certificate. The cutoff is cleared only once issuance succeeds, so a
   * refused reauthentication leaves the endpoint blocked.
   */
  const reauthenticate = async (): Promise<SshEndpoint | null> => {
    const { endpoint } = stateRef.current;
    const instanceId = leaseRef.current?.instanceId ?? endpoint?.instance_id;
    if (!endpoint || !instanceId) return null;
    setState((s) => ({ ...s, step: "Reauthenticating", error: null }));
    try {
      return adopt(
        await reauthenticateManagedInstance(managedDeps(), {
          instanceId,
          endpointId: endpoint.id,
          keyReference: DEVICE_KEYSTORE_KEY_REFERENCE,
        }),
      );
    } catch (err) {
      setState((s) => ({ ...s, step: null, error: errorMessage(err) }));
      return null;
    }
  };

  /**
   * Re-checks a managed connection against the control plane after the app
   * resumes. Timers do not run while the OS suspends the app, so the
   * certificate may be expired or due for renewal, and the instance may
   * have been suspended or replaced. Any of those means connecting again.
   * A cut-off endpoint is left alone: only an explicit reauthentication
   * restores access.
   */
  const refreshAfterResume = async (): Promise<ResumeOutcome> => {
    const { status, endpoint, choice } = stateRef.current;
    if (!endpoint || choice?.kind !== "managed") return "unchanged";
    if (status === "connecting") return "unchanged";
    if (useRemoteCutoffStore.getState().cutoffs[endpoint.id]) return "failed";
    // An earlier attempt failed (e.g. the network dropped mid-connect):
    // coming back is the moment to try again.
    if (status !== "connected") {
      return (await connectManaged()) ? "reconnected" : "failed";
    }
    try {
      const current = await managedDeps().getInstanceStatus();
      const lease = leaseRef.current;
      const generation =
        endpoint.source.type === "managed" ? endpoint.source.generation : null;
      const stale =
        !lease ||
        renewalDelayMs(lease.issuedAt, lease.expiresAt, Date.now()) === 0 ||
        current.instance?.status !== "ready" ||
        (current.endpoint !== null && current.endpoint.id !== endpoint.id) ||
        (generation !== null && current.instance.generation !== generation);
      if (!stale) return "unchanged";
      return (await connectManaged()) ? "reconnected" : "failed";
    } catch (err) {
      fail(err);
      return "failed";
    }
  };

  const disconnect = () => {
    stopRenewal();
    leaseRef.current = null;
    setState(INITIAL_STATE);
  };

  return {
    state,
    connectManaged,
    connectUserManaged,
    reauthenticate,
    refreshAfterResume,
    disconnect,
  };
}
