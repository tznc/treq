import { useEffect, useRef, useState } from "react";
import { dispatchOverSsh } from "../lib/remote-dispatch";
import type { RemoteRepoProbe, SshEndpoint } from "../lib/api-types-remote";
import { listUserManagedEndpoints } from "../lib/remote-endpoints";
import { upsertSavedRemoteRepository } from "../lib/remote-repository";
import {
  clearMobileSession,
  loadMobileSession,
  saveMobileSession,
  type MobileSessionSnapshot,
} from "../lib/mobile-session";
import { useAppResume } from "../hooks/useAppResume";
import { useMobileRemoteConnection } from "../hooks/useMobileRemoteConnection";
import { useAuthStore } from "../stores/authStore";
import { useRemoteCutoffStore } from "../stores/remoteCutoffStore";
import {
  RemoteRepoScreen,
  type RemoteRepoScreenState,
} from "./RemoteRepoScreen";
import {
  RemoteStatusBanner,
  connectionStateFromInstanceState,
  type RemoteConnectionState,
} from "./remote/RemoteStatusBanner";
import {
  MobileEndpointPicker,
  MobileRepoPicker,
} from "./mobile/MobileRemotePickers";

/**
 * After this long in the background, a resumed app reloads every remote
 * screen from the VM even when the connection itself still checks out. A
 * shorter trip away (a notification, the app switcher) keeps an open
 * terminal or form as it was.
 */
const REMOUNT_AFTER_HIDDEN_MS = 30_000;

interface OpenRepo {
  path: string;
  /** Screen to open when the repository view mounts. */
  screen: RemoteRepoScreenState | null;
}

/**
 * The mobile shell's remote flow (mobile PRD, "Mobile product behavior"):
 * pick an endpoint, pick a repository on it, then review, run agents and
 * open terminals through `RemoteRepoScreen`.
 *
 * The endpoint, repository path and screen are remembered on the device.
 * On launch, and again when the app resumes, the panel reconnects and
 * re-reads everything from the control plane and the VM instead of trusting
 * what it had in memory (acceptance criterion 7). A credential cutoff hides
 * every remote screen until the user reauthenticates (criterion 8).
 */
export function RemoteConnectPanel() {
  const connection = useMobileRemoteConnection();
  const { state } = connection;
  const user = useAuthStore((s) => s.user);
  const cutoffReason = useRemoteCutoffStore((s) =>
    state.endpoint ? s.cutoffs[state.endpoint.id] : undefined,
  );

  const [openRepo, setOpenRepo] = useState<OpenRepo | null>(null);
  const [repoBusy, setRepoBusy] = useState(false);
  const [repoError, setRepoError] = useState<string | null>(null);
  // Bumped to remount the repository view, which makes every screen in it
  // fetch again from the VM.
  const [epoch, setEpoch] = useState(0);

  const openRepository = async (
    endpoint: SshEndpoint,
    path: string,
    screen: RemoteRepoScreenState | null,
  ) => {
    setRepoBusy(true);
    setRepoError(null);
    try {
      const probe = await dispatchOverSsh<RemoteRepoProbe>(endpoint, {
        kind: "ProbeRepo",
        repo: path,
      });
      if (!probe.exists || !probe.is_repo) {
        setRepoError(`No repository found at ${path} on this host.`);
        return;
      }
      setOpenRepo({ path, screen });
      // Remember it for the repository list next time. Failing to save
      // does not stop the user from working in it now.
      void upsertSavedRemoteRepository({
        endpoint_id: endpoint.id,
        endpoint_generation:
          endpoint.source.type === "managed" ? endpoint.source.generation : 0,
        remote_path: path,
      }).catch(() => {});
    } catch (err) {
      setRepoError(err instanceof Error ? err.message : String(err));
    } finally {
      setRepoBusy(false);
    }
  };

  // Restore the remembered session once. A managed session waits until the
  // user is signed in, since the control plane needs the account.
  const [snapshot] = useState(loadMobileSession);
  const restoreStartedRef = useRef(false);
  // While a restore runs, the connection comes up before the repository is
  // probed; saving in between would forget the remembered repository if
  // the probe then fails. The saved snapshot already holds what a
  // successful restore opens, so skipping those saves loses nothing.
  const restoringRef = useRef(false);
  useEffect(() => {
    if (!snapshot || restoreStartedRef.current) return;
    if (snapshot.endpoint.kind === "managed" && !user) return;
    restoreStartedRef.current = true;
    restoringRef.current = true;
    void (async () => {
      let endpoint: SshEndpoint | null = null;
      if (snapshot.endpoint.kind === "managed") {
        endpoint = await connection.connectManaged();
      } else {
        const { id } = snapshot.endpoint;
        const record = (await listUserManagedEndpoints()).find(
          (item) => item.id === id,
        );
        if (record) endpoint = connection.connectUserManaged(record);
      }
      if (endpoint && snapshot.repoPath) {
        await openRepository(endpoint, snapshot.repoPath, snapshot.screen);
      }
    })().finally(() => {
      restoringRef.current = false;
    });
    // `connection` and `openRepository` are recreated every render; this
    // runs once, when the snapshot and the signed-in user are known.
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [snapshot, user]);

  // Persist identifiers only (see `lib/mobile-session.ts`).
  useEffect(() => {
    if (restoringRef.current || !state.choice || state.status !== "connected")
      return;
    const next: MobileSessionSnapshot = {
      endpoint: state.choice,
      repoPath: openRepo?.path ?? null,
      screen: openRepo?.screen ?? null,
    };
    saveMobileSession(next);
  }, [state.choice, state.status, openRepo]);

  useAppResume(({ reason, hiddenMs }) => {
    void (async () => {
      const outcome = await connection.refreshAfterResume();
      if (
        outcome === "reconnected" ||
        reason === "online" ||
        hiddenMs >= REMOUNT_AFTER_HIDDEN_MS
      ) {
        setEpoch((value) => value + 1);
      }
    })();
  });

  // Reload every remote screen; after a failed connect, connect again first.
  const refresh = () => {
    if (state.status !== "connected" && state.choice?.kind === "managed") {
      void connection
        .connectManaged()
        .then((endpoint) => endpoint && setEpoch((value) => value + 1));
      return;
    }
    setEpoch((value) => value + 1);
  };

  const switchEndpoint = () => {
    connection.disconnect();
    setOpenRepo(null);
    setRepoError(null);
    clearMobileSession();
  };

  const bannerState: RemoteConnectionState = cutoffReason
    ? "cutoff"
    : state.status === "connecting"
      ? "connecting"
      : state.choice?.kind === "managed"
        ? connectionStateFromInstanceState(
            state.instanceState ?? undefined,
            state.status === "connected",
          )
        : "online";

  return (
    <section className="flex flex-col gap-3">
      <h2 className="text-sm font-semibold">Remote</h2>

      {!state.endpoint && state.status !== "secure_storage_unavailable" && (
        <MobileEndpointPicker
          signedIn={Boolean(user)}
          connecting={state.status === "connecting"}
          onConnectManaged={() => void connection.connectManaged()}
          onConnectUserManaged={(record) => {
            setOpenRepo(null);
            connection.connectUserManaged(record);
          }}
        />
      )}
      {state.step && (
        <p className="text-sm text-muted-foreground">{state.step}...</p>
      )}
      {state.status === "secure_storage_unavailable" ? (
        <SecureStorageAlert
          error={state.error}
          onRetry={() => void connection.connectManaged()}
        />
      ) : (
        state.error && <p className="text-sm text-destructive">{state.error}</p>
      )}

      {state.endpoint && (
        <div className="flex flex-col gap-3">
          <div className="flex items-center justify-between gap-2">
            <p className="truncate text-sm text-muted-foreground">
              Connected to {state.endpoint.hostname}:{state.endpoint.port}
            </p>
            <button
              type="button"
              onClick={switchEndpoint}
              className="shrink-0 rounded-md border px-2 py-1 text-xs"
            >
              Switch host
            </button>
          </div>
          <RemoteStatusBanner
            state={bannerState}
            detail={
              cutoffReason
                ? `Access ended (${cutoffReason.split("_").join(" ")}).`
                : undefined
            }
            onWake={() => void connection.connectManaged()}
            onReconnect={() => void connection.connectManaged()}
            onRefresh={cutoffReason ? undefined : refresh}
          />
          {cutoffReason ? (
            <CutoffBlock
              busy={Boolean(state.step)}
              onReauthenticate={() => void connection.reauthenticate()}
            />
          ) : (
            state.status === "connected" &&
            (openRepo ? (
              <div className="flex flex-col gap-3">
                <button
                  type="button"
                  onClick={() => setOpenRepo(null)}
                  className="self-start text-xs text-muted-foreground"
                >
                  {openRepo.path} · Change repository
                </button>
                <RemoteRepoScreen
                  key={epoch}
                  endpoint={state.endpoint}
                  repo={openRepo.path}
                  initialScreen={openRepo.screen ?? undefined}
                  onScreenChange={(screen) =>
                    setOpenRepo((current) =>
                      current ? { ...current, screen } : current,
                    )
                  }
                />
              </div>
            ) : (
              <MobileRepoPicker
                endpoint={state.endpoint}
                busy={repoBusy}
                error={repoError}
                onOpen={(path) => {
                  if (state.endpoint) {
                    void openRepository(state.endpoint, path, null);
                  }
                }}
              />
            ))
          )}
        </div>
      )}
    </section>
  );
}

function CutoffBlock({
  busy,
  onReauthenticate,
}: {
  busy: boolean;
  onReauthenticate: () => void;
}) {
  return (
    <div
      role="alert"
      className="flex flex-col gap-2 rounded-md border border-destructive/40 bg-destructive/5 p-3 text-sm"
    >
      <p className="font-medium text-destructive">Remote access is blocked</p>
      <p className="text-muted-foreground">
        This device&apos;s certificate for the instance was revoked or has
        expired. Reauthenticate to get a new one before continuing.
      </p>
      <button
        type="button"
        onClick={onReauthenticate}
        disabled={busy}
        className="self-start rounded-md border px-3 py-2 text-sm"
      >
        Reauthenticate
      </button>
    </div>
  );
}

function SecureStorageAlert({
  error,
  onRetry,
}: {
  error: string | null;
  onRetry: () => void;
}) {
  return (
    <div
      role="alert"
      className="flex flex-col gap-2 rounded-md border border-destructive/40 bg-destructive/5 p-3 text-sm"
    >
      <p className="font-medium text-destructive">
        Secure storage isn&apos;t set up on this device
      </p>
      <p className="text-muted-foreground">
        Treq stores this device&apos;s connection key in your device&apos;s
        secure keystore, which requires biometrics (Face ID, Touch ID, or
        fingerprint unlock) to be enrolled. Set up biometrics in your device
        settings, then try again.
      </p>
      {error && <p className="text-xs text-muted-foreground">{error}</p>}
      <button
        type="button"
        onClick={onRetry}
        className="self-start rounded-md border px-3 py-2 text-sm"
      >
        Try again
      </button>
    </div>
  );
}
