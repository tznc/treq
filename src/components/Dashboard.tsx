/* eslint-disable max-lines */

import { syncRepoUrlParam } from "../lib/repo-url";
import { listen } from "@tauri-apps/api/event";
import { WebviewWindow } from "@tauri-apps/api/webviewWindow";
import { ask } from "@tauri-apps/plugin-dialog";
import {
  type CSSProperties,
  useEffect,
  useMemo,
  useRef,
  useState,
} from "react";
import useSWR from "swr";
import { useLocation } from "wouter";
import { useAutoUpdate } from "../hooks/useAutoUpdate";
import { useKeyboardShortcut } from "../hooks/useKeyboard";
import { useMutation } from "../hooks/useMutation";
import { useWorkspaceHierarchy } from "../hooks/useWorkspaceHierarchy";
import {
  type AgentDeepLinkRequest,
  findWorkspaceByBranch,
  isProcessedAgentRequest,
  markProcessedAgentRequest,
  parseAgentDeepLinks,
  popPendingAgentRequests,
  processAgentDeepLinkRequests,
} from "../lib/agentDeepLink";
import {
  acknowledgeAgentDispatch,
  addPromptHistory,
  archiveWorkspace,
  checkAndRebaseWorkspaces,
  createSession,
  deleteWorkspace,
  getRepoCurrentBranch,
  getRepoDefaultBranch,
  getRepoSetting,
  getSessions,
  getSetting,
  getWorkspaces,
  initRepo,
  listLocalSshIdentities,
  listRepoBranches,
  listSshHosts,
  listWorkspaceStatuses,
  moveWorkspaceChanges,
  readLocalSshPublicKey,
  selectFolder,
  setSetting,
  setWindowRepoPath,
  updateSessionAccess,
  type Workspace,
} from "../lib/api";
import type { RemoteRepository } from "../lib/api-types";
import type {
  InstanceStatusResponse,
  MachineUsageReport,
  RemoteRepoProbe,
  RemoteAgentId,
  RepositoryInspection,
  SshEndpoint,
} from "../lib/api-types-remote";
import { RemoteAmbiguousMutationDialog } from "./remote/RemoteAmbiguousMutationDialog";
import { RemoteCapabilityNotice } from "./remote/RemoteCapabilityNotice";
import {
  activeRepositoryFromRemote,
  localActiveRepository,
  repositoryCacheKey,
  type PersistedRemoteRepository,
} from "../lib/active-repository";
import { ActiveRepositoryProvider } from "../lib/active-repository-context";
import { capabilitiesFor } from "../lib/remote-capabilities";
import { invalidateRemoteRepositoryData } from "../lib/remote-mutation-ui";
import {
  useRemoteAgentRefresh,
  useRemoteChangeMarkerWatch,
} from "../hooks/useRemoteRefresh";
import { ARTIFACTS_BASE_PATH, artifactsPath } from "../lib/artifactRoutes";
import {
  type ChangeFilesMoveRequest,
  dispatchRefreshWorkspaceChanges,
} from "../lib/change-file-drag";
import {
  GITHUB_BASE_PATH,
  githubDetailPath,
  githubListPath,
  stateFilterForPrState,
} from "../lib/githubRoutes";
import {
  autoReviewSummary,
  prepareWorkspaceReview,
  type AutoReviewEvent,
} from "../lib/agent-review-launch";
import { LINEAR_BASE_PATH } from "../lib/linearRoutes";
import { useLinearAutoKickoff } from "../hooks/useLinearAutoKickoff";
import { useTrackerAutoKickoff } from "../hooks/useTrackerAutoKickoff";
import { TRACKER_PROVIDERS, type TrackerProvider } from "../lib/trackers";
import { openRepositoryAtPath as openRepositoryAtPathShared } from "../lib/open-repository";
import {
  issueFromGitHub,
  issueFromLinear,
  issueFromTrackerItem,
  type IssueAttachment,
} from "../lib/promptAttachments";
import {
  deleteInstance as deleteManagedInstance,
  ensureInstance,
  getInstanceStatus,
  MANAGED_REPOSITORIES_ROOT,
  issueCertificate,
  registerClientKey,
  reprovisionInstance,
  revokeClientKey,
  wakeInstance,
} from "../lib/remote-control-plane";
import { isProRequiredError } from "../lib/pro-required";
import {
  listUserManagedEndpoints,
  saveUserManagedEndpoint,
  sshEndpointFromUserManaged,
  type SavedRemoteRepositoryRecord,
  type UserManagedEndpointRecord,
} from "../lib/remote-endpoints";
import {
  dispatchMutationOverSsh,
  dispatchOverSsh,
} from "../lib/remote-dispatch";
import {
  LAST_OPENED_REMOTE_REPO_ID_KEY,
  canonicalizeRemotePath,
  getSavedRemoteRepository,
  listSavedRepositoriesForEndpoint,
  rememberLastOpenedRemoteRepository,
  restoreSavedRemoteRepository,
  upsertSavedRemoteRepository,
} from "../lib/remote-repository";
import {
  connectExistingReadyInstance,
  connectManagedInstance,
  pickDefaultKeyReference,
  reauthenticateManagedInstance,
  wakeManagedInstance,
  type ManagedConnectionDeps,
  type ManagedConnectionResult,
  type RenewalController,
} from "../lib/managed-ssh-connection";
import { startManagedCertificateRenewal } from "../lib/remote-cert-lifecycle";
import { ensureRelayAccessTokenSync } from "../lib/remote-relay-auth";
import {
  newRemoteSessionLabel,
  remoteAgentIdFor,
  remoteLaunchFor,
  resolveRemoteTerminalTarget,
} from "../lib/remote-terminal-target";
import type { RemoteTerminalTarget } from "./RemoteTerminalPanel";
import { useRemoteCutoffStore } from "../stores/remoteCutoffStore";
import { remoteForceCutoff } from "../lib/api-extra";
import { RemoteRepositorySelector } from "./remote/RemoteRepositorySelector";
import { RemoteTerminalDialog } from "./RemoteTerminalDialog";
import { invalidateReviewChangeCount } from "../lib/review-change-count";
import {
  clearSWRCache,
  fetchAndCache,
  invalidateQueries,
  pollMs,
} from "../lib/swr-cache";
import { getFullWorkspacePath } from "../lib/utils";
import {
  buildWorkspaceTree,
  flattenWorkspaceTree,
} from "../lib/workspace-tree";
import {
  useFeaturePreviewStore,
  usePreviewFeature,
} from "../stores/featurePreviewStore";
import { useSidebarWidthStore } from "../stores/sidebarWidthStore";
import { AgentPromptDialog } from "./AgentPromptDialog";
import { ArtifactsPage } from "./ArtifactsPage";
import { CommandPalette } from "./CommandPalette";
import { ErrorBoundary } from "./ErrorBoundary";
import { GitHubPanel } from "./GitHubPanel";
import { KeyboardShortcutsModal } from "./KeyboardShortcutsModal";
import { LinearPanel } from "./LinearPanel";
import { TrackerPanel } from "./TrackerPanel";
import { MergePreviewPage } from "./MergePreviewPage";
import { Onboarding } from "./Onboarding";
import { PromptHistoryModal } from "./PromptHistoryModal";
import {
  RemoteSetupDialog,
  type UserManagedFormValues,
} from "./remote/RemoteSetupDialog";
import {
  connectionStateFromInstanceState,
  RemoteStatusBanner,
} from "./remote/RemoteStatusBanner";
import { SettingsPage } from "./SettingsPage";
import { ShowWorkspace } from "./ShowWorkspace";
import { StashModal } from "./StashModal";
import type { BranchListItem } from "./TargetBranchSelector";
import type {
  AgentSessionData,
  TerminalSessionSummary,
} from "./terminal/types";
import {
  UnifiedWorkspaceDialog,
  type WorkspaceDialogDefaults,
} from "./UnifiedWorkspaceDialog";
import {
  AlertDialog,
  AlertDialogAction,
  AlertDialogCancel,
  AlertDialogContent,
  AlertDialogDescription,
  AlertDialogFooter,
  AlertDialogHeader,
  AlertDialogTitle,
} from "./ui/alert-dialog";
import { Button } from "./ui/button";
import { SidebarInset, SidebarProvider } from "./ui/sidebar";
import { useToast } from "./ui/toast";
import { WorkspacePicker } from "./WorkspacePicker";
import { WorkspaceSidebar } from "./WorkspaceSidebar";
import {
  WorkspaceTerminalPane,
  type WorkspaceTerminalPaneHandle,
} from "./WorkspaceTerminalPane";
import {
  agentInfo,
  DEFAULT_AGENT,
  toAgentKind,
  type AgentKind,
} from "../lib/agents";
import { agentTerminalId } from "./terminal/agentTerminalId";

function generationFromEndpoint(
  endpoint: SshEndpoint,
  fallback: number,
): number {
  if (endpoint.source.type === "managed") return endpoint.source.generation;
  return fallback;
}

type ViewMode =
  | "session"
  | "show-workspace"
  | "settings"
  | "artifacts"
  | "merge-preview"
  | "github"
  | "linear"
  | TrackerProvider;

type SessionOpenOptions = {
  initialPrompt?: string;
  promptLabel?: string;
  forceNew?: boolean;
  sessionName?: string;
  selectedFilePath?: string;
};

interface DashboardProps {
  initialViewMode?: ViewMode;
}
export const Dashboard: React.FC<DashboardProps> = ({
  initialViewMode = "show-workspace",
}) => {
  const [repoPath, setRepoPath] = useState(() => {
    const urlParams = new URLSearchParams(window.location.search);
    return urlParams.get("repo") || "";
  });
  useEffect(() => {
    syncRepoUrlParam(repoPath);
  }, [repoPath]);
  const [currentBranch, setCurrentBranch] = useState<string | null>(null);
  const [homeRepoDisplayRef, setHomeRepoDisplayRef] = useState<string | null>(
    null,
  );
  const [unifiedDialogDefaults, setUnifiedDialogDefaults] =
    useState<WorkspaceDialogDefaults | null>(null);
  const [pendingChangeMove, setPendingChangeMove] = useState<{
    files: string[];
    sourceBranch: string;
    destinationBranch: string;
    destinationLabel: string;
  } | null>(null);
  const [changeMovePending, setChangeMovePending] = useState(false);
  const [viewMode, setViewMode] = useState<ViewMode>(initialViewMode);
  const [location, navigate] = useLocation();
  const remoteSshEnabled = usePreviewFeature("remoteSsh");
  const linearIntegrationEnabled = usePreviewFeature("linearIntegration");
  const trelloIntegrationEnabled = usePreviewFeature("trelloIntegration");
  const jiraIntegrationEnabled = usePreviewFeature("jiraIntegration");
  const trackerEnabled: Record<TrackerProvider, boolean> = {
    trello: trelloIntegrationEnabled,
    jira: jiraIntegrationEnabled,
  };
  const previousViewModeRef = useRef<ViewMode>(
    initialViewMode === "settings" ? "show-workspace" : initialViewMode,
  );
  const [selectedWorkspace, setSelectedWorkspace] = useState<Workspace | null>(
    null,
  );
  const [mergeWorkspace, setMergeWorkspace] = useState<Workspace | null>(null);
  const [showCommandPalette, setShowCommandPalette] = useState(false);
  const [showAgentPromptDialog, setShowAgentPromptDialog] = useState(false);
  const [showPromptHistory, setShowPromptHistory] = useState(false);
  const [promptHistoryFocusId, setPromptHistoryFocusId] = useState<
    number | null
  >(null);
  const [showStashModal, setShowStashModal] = useState(false);
  const [runPromptRequest, setRunPromptRequest] = useState<{
    prompt?: string;
    workspaceId: number | null;
    issue?: IssueAttachment | null;
  } | null>(null);
  const [showBranchSwitcher, setShowBranchSwitcher] = useState(false);
  const [showFilePicker, setShowFilePicker] = useState(false);
  const [showKeyboardShortcuts, setShowKeyboardShortcuts] = useState(false);
  const [showWorkspacePicker, setShowWorkspacePicker] = useState(false);
  const [showWorkspaceDeletion, setShowWorkspaceDeletion] = useState(false);
  const [activeSessionId, setActiveSessionId] = useState<number | null>(null);
  const [pendingSessionData, setPendingSessionData] = useState<
    Map<
      number,
      {
        pendingPrompt?: string;
        permissionMode?: "plan" | "acceptEdits";
        agent?: AgentKind;
        workspacePath?: string | null;
      }
    >
  >(new Map());
  const [sessionSelectedFile, setSessionSelectedFile] = useState<string | null>(
    null,
  );
  const [selectedWorkspaceIds, setSelectedWorkspaceIds] = useState<Set<number>>(
    new Set(),
  );
  const [archivingWorkspaceIds, setArchivingWorkspaceIds] = useState<
    Set<number>
  >(new Set());
  const [exitingWorkspaceIds, setExitingWorkspaceIds] = useState<Set<number>>(
    new Set(),
  );
  const lastSelectedWorkspaceIndexRef = useRef<number | null>(null);
  const [deferredAgentRequests, setDeferredAgentRequests] = useState<
    AgentDeepLinkRequest[]
  >([]);
  const [, setShowWorkspaceActiveTab] = useState("overview");
  const [terminalSessionSummaries, setTerminalSessionSummaries] = useState<
    TerminalSessionSummary[]
  >([]);
  const [remoteSshHosts, setRemoteSshHosts] = useState<string[]>([]);
  const [activeRemoteRepo, setActiveRemoteRepo] =
    useState<RemoteRepository | null>(null);

  // -- Remote setup flow (managed + user-owned endpoints) ------------------
  const [showRemoteSetupDialog, setShowRemoteSetupDialog] = useState(false);
  const [instanceStatus, setInstanceStatus] =
    useState<InstanceStatusResponse | null>(null);
  const [provisioningStage, setProvisioningStage] = useState<string>();
  const [provisioningError, setProvisioningError] = useState<string>();
  const [cloudUsage, setCloudUsage] = useState<MachineUsageReport>();
  const [cloudUsageError, setCloudUsageError] = useState<string>();
  // Explicit user-managed or managed endpoints carry a native SSH identity.
  // Alias-backed repositories still use the same workspace tree; they
  // dispatch through `remote_dispatch_local` until an endpoint with a
  // pinned host key is attached (native SSH does not resolve ~/.ssh/config).
  const [activeSshEndpoint, setActiveSshEndpoint] =
    useState<SshEndpoint | null>(null);
  const [activeEndpointGeneration, setActiveEndpointGeneration] = useState(0);
  const [explicitEndpointRepoPath, setExplicitEndpointRepoPath] =
    useState("~/src/project");
  const [explicitEndpointError, setExplicitEndpointError] = useState<string>();
  const [explicitEndpointRepoConnected, setExplicitEndpointRepoConnected] =
    useState(false);
  const [savedRemoteRepos, setSavedRemoteRepos] = useState<
    SavedRemoteRepositoryRecord[]
  >([]);
  const [selectedSavedRepoId, setSelectedSavedRepoId] = useState<string | null>(
    null,
  );
  const [explicitEndpointProbe, setExplicitEndpointProbe] =
    useState<RemoteRepoProbe | null>(null);
  const [explicitEndpointCloneUrl, setExplicitEndpointCloneUrl] = useState("");
  const [confirmInitRemoteRepo, setConfirmInitRemoteRepo] = useState(false);
  const [showRemoteTerminalDialog, setShowRemoteTerminalDialog] =
    useState(false);
  const [explicitGenerationTransition, setExplicitGenerationTransition] =
    useState(false);
  const [remoteRepoBusy, setRemoteRepoBusy] = useState(false);

  const refreshSavedRemoteRepos = async (
    endpointId: string,
    generation: number,
  ) => {
    setSavedRemoteRepos(
      await listSavedRepositoriesForEndpoint(endpointId, generation),
    );
  };

  // Builds the `PersistedRemoteRepository` the workspace tree (via
  // `activeRepository`/`ActiveRepositoryProvider`) keys off of, from a saved
  // descriptor plus the inspection that just proved the path is a real
  // repository. Only called once reconnect + trust + inspect have all
  // succeeded (see `restoreSavedRemoteRepository` and the handlers below).
  const connectedRepoFromDescriptor = (
    descriptor: SavedRemoteRepositoryRecord,
    endpoint: SshEndpoint,
    inspection?: RepositoryInspection,
  ): PersistedRemoteRepository => ({
    host: endpoint.hostname,
    path: descriptor.canonical_remote_path,
    display_name: descriptor.display_name,
    repo_uri: `ssh://${endpoint.username}@${endpoint.hostname}:${endpoint.port}${descriptor.canonical_remote_path}`,
    inspection: inspection ?? {
      root: descriptor.canonical_remote_path,
      repository_type: "jj",
      current_branch: null,
      default_branch: "main",
      current_change_id: "",
      current_commit_id: "",
      descriptor: {
        id: descriptor.id,
        location: {
          type: "ssh",
          host: endpoint.hostname,
          path: descriptor.canonical_remote_path,
        },
        display_name: descriptor.display_name,
      },
    },
    endpoint,
    endpoint_id: descriptor.endpoint_id,
    endpoint_generation: descriptor.endpoint_generation,
  });

  const markRemoteRepoOpen = async (
    descriptor: SavedRemoteRepositoryRecord,
    endpoint?: SshEndpoint,
    inspection?: RepositoryInspection,
  ) => {
    await rememberLastOpenedRemoteRepository(descriptor.id);
    setSelectedSavedRepoId(descriptor.id);
    setExplicitEndpointRepoPath(descriptor.canonical_remote_path);
    setExplicitEndpointRepoConnected(true);
    await refreshSavedRemoteRepos(
      descriptor.endpoint_id,
      descriptor.endpoint_generation,
    );
    const connectionEndpoint = endpoint ?? activeSshEndpoint;
    if (connectionEndpoint) {
      const connectedRepo = connectedRepoFromDescriptor(
        descriptor,
        connectionEndpoint,
        inspection,
      );
      setActiveRemoteRepo(connectedRepo);
    }
  };

  const inspectAndRegisterPath = async (
    endpoint: SshEndpoint,
    generation: number,
    path: string,
  ) => {
    const canonical = canonicalizeRemotePath(path);
    const inspection = await dispatchOverSsh<RepositoryInspection>(endpoint, {
      kind: "InspectRepository",
      repo: canonical,
    });
    const descriptor = await upsertSavedRemoteRepository({
      endpoint_id: endpoint.id,
      endpoint_generation: generation,
      remote_path: canonical,
      last_successful_trust_validation: new Date().toISOString(),
    });
    await markRemoteRepoOpen(descriptor, endpoint, inspection);
  };

  // The currently registered client key for the managed instance, so a
  // revocation of *this* key (PRD "Revoking the active key must immediately
  // cut off the active endpoint") and reauthentication after a hard cutoff
  // both know which local identity to use.
  const [managedKeyId, setManagedKeyId] = useState<string | null>(null);
  // Last local identity reference used to connect the managed instance, so
  // wake/reconnect and reauthentication can re-run the same
  // register-key -> issue-certificate sequence without asking the user to
  // reselect their identity every time.
  const [selectedKeyReference, setSelectedKeyReference] = useState<
    string | null
  >(null);
  // The renewal loop for the active managed endpoint, tagged with that
  // endpoint's id so a certificate update (same id) keeps it running while a
  // replacement or disconnect (different id) stops it.
  const renewalRef = useRef<{
    endpointId: string;
    controller: RenewalController;
  } | null>(null);
  const cutoffs = useRemoteCutoffStore((s) => s.cutoffs);
  const clearRemoteCutoff = useRemoteCutoffStore((s) => s.clearCutoff);

  useEffect(() => {
    void useRemoteCutoffStore.getState().startListening();
    return () => useRemoteCutoffStore.getState().stopListening();
  }, []);

  // Stop renewal on disconnect or endpoint replacement (PRD "Stop renewal on
  // disconnect or endpoint replacement"). Keyed on the endpoint id, not the
  // object: a renewed certificate produces a new endpoint object with the
  // same id, and that must not stop the loop that produced it. The check
  // also cannot stop a loop that was started for the endpoint just
  // activated, whichever of the render or the handler's bookkeeping runs
  // first.
  const activeSshEndpointId = activeSshEndpoint?.id ?? null;
  useEffect(() => {
    const { current } = renewalRef;
    if (current && current.endpointId !== activeSshEndpointId) {
      current.controller.stop();
      renewalRef.current = null;
    }
  }, [activeSshEndpointId]);
  useEffect(
    () => () => {
      renewalRef.current?.controller.stop();
      renewalRef.current = null;
    },
    [],
  );

  const managedConnectionDeps = (): ManagedConnectionDeps => ({
    readPublicKey: readLocalSshPublicKey,
    registerClientKey: async (publicKey, comment, idempotencyKey) => {
      const key = await registerClientKey({
        public_key: publicKey,
        comment,
        idempotency_key: idempotencyKey,
      });
      setManagedKeyId(key.id);
      return key;
    },
    ensureInstance: (_region, _size, idempotencyKey) =>
      ensureInstance({
        idempotency_key: idempotencyKey,
      }),
    getInstanceStatus,
    issueCertificate: (instanceId, keyId) =>
      issueCertificate({ instance_id: instanceId, key_id: keyId }),
    activateEndpoint: (endpoint) => {
      renewalRef.current?.controller.stop();
      renewalRef.current = null;
      setActiveSshEndpoint(endpoint);
      setActiveEndpointGeneration(
        endpoint.source.type === "managed" ? endpoint.source.generation : 0,
      );
    },
    // Only swap the certificate if this endpoint is still the active one;
    // the user may have switched to another endpoint since renewal began.
    updateEndpointCertificate: (endpoint) =>
      setActiveSshEndpoint((current) =>
        current?.id === endpoint.id ? endpoint : current,
      ),
    wakeInstance: (instanceId, idempotencyKey) =>
      wakeInstance({
        instance_id: instanceId,
        idempotency_key: idempotencyKey,
      }),
    startRenewal: (lease, onRenewed) =>
      startManagedCertificateRenewal(lease, onRenewed),
    prepareRelay: () => ensureRelayAccessTokenSync(),
    clearCutoff: (endpointId) => clearRemoteCutoff(endpointId),
  });

  const handleConnectExplicitEndpointRepo = async () => {
    if (!activeSshEndpoint) return;
    setExplicitEndpointError(undefined);
    try {
      const probe = await dispatchOverSsh<RemoteRepoProbe>(activeSshEndpoint, {
        kind: "ProbeRepo",
        repo: explicitEndpointRepoPath,
      });
      setExplicitEndpointProbe(probe);
      if (!probe.is_repo) {
        setExplicitEndpointError(
          probe.needs_clone
            ? "No repository at that path. Clone or initialize it."
            : "No repository found at that path on the remote machine.",
        );
        return;
      }
      await inspectAndRegisterPath(
        activeSshEndpoint,
        activeEndpointGeneration,
        explicitEndpointRepoPath,
      );
    } catch (error) {
      setExplicitEndpointError(
        error instanceof Error ? error.message : String(error),
      );
    }
  };

  const handleProbeExplicitEndpointRepo = async () => {
    if (!activeSshEndpoint) return;
    setExplicitEndpointError(undefined);
    setRemoteRepoBusy(true);
    try {
      const probe = await dispatchOverSsh<RemoteRepoProbe>(activeSshEndpoint, {
        kind: "ProbeRepo",
        repo: explicitEndpointRepoPath,
      });
      setExplicitEndpointProbe(probe);
    } catch (error) {
      setExplicitEndpointError(
        error instanceof Error ? error.message : String(error),
      );
    } finally {
      setRemoteRepoBusy(false);
    }
  };

  const handleCloneExplicitEndpointRepo = async () => {
    if (!activeSshEndpoint || !explicitEndpointCloneUrl.trim()) return;
    setExplicitEndpointError(undefined);
    setRemoteRepoBusy(true);
    try {
      const result = await dispatchMutationOverSsh<RepositoryInspection>(
        activeSshEndpoint,
        {
          kind: "CloneRepo",
          repo_url: explicitEndpointCloneUrl.trim(),
          destination: canonicalizeRemotePath(explicitEndpointRepoPath),
          idempotency_key: `clone-${activeSshEndpoint.id}-${Date.now()}`,
        },
      );
      if (result.status === "ambiguous") {
        setExplicitEndpointError(result.reason);
        return;
      }
      await inspectAndRegisterPath(
        activeSshEndpoint,
        activeEndpointGeneration,
        explicitEndpointRepoPath,
      );
    } catch (error) {
      setExplicitEndpointError(
        error instanceof Error ? error.message : String(error),
      );
    } finally {
      setRemoteRepoBusy(false);
    }
  };

  const handleInitExplicitEndpointRepo = async () => {
    if (!activeSshEndpoint || !confirmInitRemoteRepo) return;
    setExplicitEndpointError(undefined);
    setRemoteRepoBusy(true);
    try {
      const result = await dispatchMutationOverSsh<RepositoryInspection>(
        activeSshEndpoint,
        {
          kind: "InitRepo",
          repo: canonicalizeRemotePath(explicitEndpointRepoPath),
          idempotency_key: `init-${activeSshEndpoint.id}-${Date.now()}`,
        },
      );
      if (result.status === "ambiguous") {
        setExplicitEndpointError(result.reason);
        return;
      }
      await inspectAndRegisterPath(
        activeSshEndpoint,
        activeEndpointGeneration,
        explicitEndpointRepoPath,
      );
    } catch (error) {
      setExplicitEndpointError(
        error instanceof Error ? error.message : String(error),
      );
    } finally {
      setRemoteRepoBusy(false);
    }
  };

  const handleSelectSavedRemoteRepo = async (id: string) => {
    if (!activeSshEndpoint) return;
    const descriptor = savedRemoteRepos.find((repo) => repo.id === id);
    if (!descriptor) return;
    setSelectedSavedRepoId(id);
    setExplicitEndpointRepoPath(descriptor.canonical_remote_path);
    setExplicitEndpointError(undefined);
    setRemoteRepoBusy(true);
    try {
      const result = await restoreSavedRemoteRepository({
        descriptor,
        currentGeneration: activeEndpointGeneration,
        explicitGenerationTransition,
        reconnect: async () => {
          try {
            await dispatchOverSsh(activeSshEndpoint, {
              kind: "ProbeRepo",
              repo: descriptor.canonical_remote_path,
            });
            return true;
          } catch {
            return false;
          }
        },
        validateHostKey: async () =>
          activeSshEndpoint.host_keys.some(
            (key) => key.fingerprint_sha256.length > 0,
          ),
        inspect: (canonicalPath) =>
          dispatchOverSsh<RepositoryInspection>(activeSshEndpoint, {
            kind: "InspectRepository",
            repo: canonicalPath,
          }),
      });
      if (!result.ok) {
        setExplicitEndpointRepoConnected(false);
        setExplicitEndpointError(
          result.reason === "generation_mismatch"
            ? "Endpoint generation changed. Confirm a trust transition before restoring."
            : `Restore refused: ${result.reason.replace(/_/g, " ")}.`,
        );
        return;
      }
      setExplicitGenerationTransition(false);
      await markRemoteRepoOpen(
        result.descriptor,
        activeSshEndpoint,
        result.inspection,
      );
    } finally {
      setRemoteRepoBusy(false);
    }
  };

  const activeRepository = useMemo(() => {
    if (repoPath) return localActiveRepository(repoPath);
    if (activeRemoteRepo) {
      return activeRepositoryFromRemote(
        activeRemoteRepo as PersistedRemoteRepository,
        activeSshEndpoint,
      );
    }
    return null;
  }, [repoPath, activeRemoteRepo, activeSshEndpoint, activeEndpointGeneration]);

  const isRemoteActive = activeRepository?.location.type === "ssh";
  const dataRepoPath = activeRepository?.canonicalPath ?? repoPath;
  const queryRepoKey = activeRepository
    ? repositoryCacheKey(activeRepository)
    : repoPath;
  const remoteCaps = capabilitiesFor(Boolean(isRemoteActive));
  useLinearAutoKickoff(
    dataRepoPath,
    linearIntegrationEnabled && !isRemoteActive,
  );
  useTrackerAutoKickoff(
    "trello",
    dataRepoPath,
    trelloIntegrationEnabled && !isRemoteActive,
  );
  useTrackerAutoKickoff(
    "jira",
    dataRepoPath,
    jiraIntegrationEnabled && !isRemoteActive,
  );
  const cutoffReason = useRemoteCutoffStore((s) =>
    activeRepository?.endpointId
      ? s.cutoffs[activeRepository.endpointId]
      : undefined,
  );

  useRemoteChangeMarkerWatch(
    isRemoteActive ? activeRepository : null,
    selectedWorkspace?.id ?? null,
  );
  useRemoteAgentRefresh(
    isRemoteActive ? activeRepository : null,
    selectedWorkspace?.id ?? null,
  );

  const handleOpenRemoteSetup = async () => {
    setProvisioningError(undefined);
    try {
      const [hosts, status] = await Promise.all([
        listSshHosts().catch(() => []),
        getInstanceStatus().catch(() => null),
      ]);
      setRemoteSshHosts(hosts.map((h) => h.alias));
      setInstanceStatus(status);
      void loadCloudUsage(status);
    } catch {
      // Best-effort: the dialog still opens and shows what it could load.
    }
    setShowRemoteSetupDialog(true);
  };

  const refreshInstanceStatus = async () => {
    try {
      setInstanceStatus(await getInstanceStatus());
    } catch {
      // Leave the last-known status in place; the banner reflects staleness.
    }
  };

  // Usage is computed on the machine itself over managed SSH, so it is only
  // asked for once the cloud workspace is ready and has an endpoint.
  const loadCloudUsage = async (status: InstanceStatusResponse | null) => {
    const serverEndpoint = status?.endpoint;
    if (status?.instance?.status !== "ready" || !serverEndpoint) return;
    // Prefer the active endpoint when it carries this device's key and
    // certificate for the same instance; the control plane's endpoint names
    // a server-side key id the native transport cannot authenticate with.
    const endpoint =
      activeSshEndpoint?.id === serverEndpoint.id &&
      activeSshEndpoint.authentication.type === "certificate" &&
      activeSshEndpoint.authentication.certificate
        ? activeSshEndpoint
        : serverEndpoint;
    setCloudUsage(undefined);
    setCloudUsageError(undefined);
    try {
      setCloudUsage(
        await dispatchOverSsh<MachineUsageReport>(endpoint, {
          kind: "MachineUsage",
          root: MANAGED_REPOSITORIES_ROOT,
        }),
      );
    } catch (error) {
      setCloudUsageError(
        error instanceof Error ? error.message : String(error),
      );
    }
  };

  const refreshCloudWorkspace = async () => {
    setProvisioningError(undefined);
    const status = await getInstanceStatus().catch(() => null);
    if (status) setInstanceStatus(status);
    await loadCloudUsage(status);
  };

  // The local identity managed SSH authenticates with: the one used last in
  // this session, else a default from `~/.ssh`. Throws a message the setup
  // panel can show when there is no key at all.
  const resolveManagedKeyReference = async (): Promise<string> => {
    if (selectedKeyReference) return selectedKeyReference;
    const reference = pickDefaultKeyReference(await listLocalSshIdentities());
    if (!reference) {
      throw new Error(
        "No SSH key found in ~/.ssh. Create one with `ssh-keygen -t ed25519`, then try again.",
      );
    }
    return reference;
  };

  // Records a successful managed connection: remembers the renewal loop for
  // this endpoint and the identity used. Connect and provision then open the
  // repository list on the certificate-authenticated endpoint; wake and
  // reauthentication leave the current repository view alone.
  const adoptManagedConnection = async (
    result: ManagedConnectionResult,
    keyReference: string,
    { openRepositories }: { openRepositories: boolean },
  ) => {
    renewalRef.current = {
      endpointId: result.endpoint.id,
      controller: result.renewal,
    };
    setSelectedKeyReference(keyReference);
    await refreshInstanceStatus();
    if (!openRepositories) return;
    setExplicitEndpointRepoConnected(false);
    setShowRemoteSetupDialog(false);
    await refreshSavedRemoteRepos(
      result.endpoint.id,
      result.endpoint.source.type === "managed"
        ? result.endpoint.source.generation
        : 0,
    );
  };

  // Full identity -> registration -> certificate -> endpoint sequence (PRD
  // "Managed VM certificate flow"), plus the initial silent-renewal start.
  // See `src/lib/managed-ssh-connection.ts` for the state machine itself.
  const handleProvisionManaged = async () => {
    setProvisioningError(undefined);
    setProvisioningStage("Requesting provisioning...");
    try {
      const keyReference = await resolveManagedKeyReference();
      // One idempotency key per click: the control plane replays the stored
      // result for a repeated key, so a key shared across clicks would turn
      // "Retry cloud workspace creation" after a failure into a replay of that
      // failure. The control plane still allows only one cloud workspace per user.
      const provisionKey = `provision-managed-${crypto.randomUUID()}`;
      const result = await connectManagedInstance(
        {
          ...managedConnectionDeps(),
          ensureInstance: () =>
            ensureInstance({ idempotency_key: provisionKey }),
          getInstanceStatus: async () => {
            const status = await getInstanceStatus();
            setInstanceStatus(status);
            setProvisioningStage(
              `Waiting for the cloud workspace (${status.instance?.status ?? "unprovisioned"})...`,
            );
            return status;
          },
        },
        { region: "us_east", size: "small", keyReference },
      );
      await adoptManagedConnection(result, keyReference, {
        openRepositories: true,
      });
    } catch (error) {
      const message = error instanceof Error ? error.message : String(error);
      setProvisioningError(message);
      // Without Pro the card shows "Upgrade to Pro" instead of an error.
      if (!isProRequiredError(error)) {
        addToast({
          title: "Couldn't create cloud workspace",
          description: message,
          type: "error",
        });
      }
      // The control plane records the failure; reload so the card offers a
      // retry instead of a fresh create.
      await refreshInstanceStatus();
    } finally {
      setProvisioningStage(undefined);
    }
  };

  // Connect flow for an existing ready managed instance, run by "Open
  // repositories" when there is no certificate-authenticated endpoint yet.
  // It still runs key registration + certificate issuance + renewal, just
  // skips provisioning/readiness polling since the instance is already
  // `ready`.
  const handleConnectManaged = async () => {
    setProvisioningError(undefined);
    try {
      const keyReference = await resolveManagedKeyReference();
      // A fresh status rather than the one the dialog opened with, so a
      // workspace that went to sleep in the meantime fails with "not ready"
      // instead of a certificate for an unreachable host.
      const status = await getInstanceStatus();
      setInstanceStatus(status);
      const result = await connectExistingReadyInstance(
        managedConnectionDeps(),
        { status, keyReference },
      );
      await adoptManagedConnection(result, keyReference, {
        openRepositories: true,
      });
    } catch (error) {
      setProvisioningError(
        error instanceof Error ? error.message : String(error),
      );
    }
  };

  // Wake/reconnect ordering (required behavior): wake, poll readiness,
  // refresh credentials (fresh certificate), validate generation/host trust
  // (via `activateEndpoint` replacing the endpoint), reconnect - only then is
  // interaction restored (the new `activeSshEndpoint` is what gates it).
  const handleWakeManaged = async () => {
    const instanceId = instanceStatus?.instance?.instance_id;
    if (!instanceId) return;
    setProvisioningError(undefined);
    try {
      const keyReference = await resolveManagedKeyReference();
      const result = await wakeManagedInstance(managedConnectionDeps(), {
        instanceId,
        keyReference,
      });
      await adoptManagedConnection(result, keyReference, {
        openRepositories: false,
      });
    } catch (error) {
      setProvisioningError(
        error instanceof Error ? error.message : String(error),
      );
    }
  };

  // Reauthentication after a hard cutoff (PRD "The user regains access only
  // by reauthenticating and obtaining a new certificate through the normal
  // registration and issuance flow"). Cutoff is cleared only once
  // `reauthenticateManagedInstance` confirms fresh certificate issuance
  // succeeded.
  const handleReauthenticateManaged = async (keyReference?: string) => {
    const resolvedKeyReference = keyReference ?? selectedKeyReference;
    if (
      !activeSshEndpoint ||
      !instanceStatus?.instance ||
      !resolvedKeyReference
    )
      return;
    setProvisioningError(undefined);
    try {
      const result = await reauthenticateManagedInstance(
        managedConnectionDeps(),
        {
          instanceId: instanceStatus.instance.instance_id,
          endpointId: activeSshEndpoint.id,
          keyReference: resolvedKeyReference,
        },
      );
      await adoptManagedConnection(result, resolvedKeyReference, {
        openRepositories: false,
      });
    } catch (error) {
      setProvisioningError(
        error instanceof Error ? error.message : String(error),
      );
    }
  };

  const handleReprovisionManaged = async () => {
    const instanceId = instanceStatus?.instance?.instance_id;
    if (!instanceId) return;
    setProvisioningError(undefined);
    try {
      await reprovisionInstance({
        instance_id: instanceId,
        idempotency_key: `reprovision-${instanceId}-${Date.now()}`,
      });
      await refreshInstanceStatus();
    } catch (error) {
      setProvisioningError(
        error instanceof Error ? error.message : String(error),
      );
    }
  };

  const handleDeleteManagedInstance = async () => {
    const instanceId = instanceStatus?.instance?.instance_id;
    if (!instanceId) return;
    setProvisioningError(undefined);
    try {
      await deleteManagedInstance({
        instance_id: instanceId,
        idempotency_key: `delete-${instanceId}-${Date.now()}`,
      });
      await refreshInstanceStatus();
    } catch (error) {
      setProvisioningError(
        error instanceof Error ? error.message : String(error),
      );
    }
  };

  // Revoking the active key must immediately cut off the active endpoint
  // (required behavior) - this is a client-driven mirror of the same forced
  // cutoff the certificate-renewal loop performs when the control plane
  // reports the key revoked on the next renewal attempt, so access is
  // blocked right away rather than only after the current certificate would
  // otherwise have been silently renewed.
  const handleRevokeKey = async (keyReference: string) => {
    setProvisioningError(undefined);
    const keyId = managedKeyId ?? keyReference;
    try {
      await revokeClientKey({
        key_id: keyId,
        idempotency_key: `revoke-${keyId}`,
      });
      if (activeSshEndpoint && managedKeyId) {
        renewalRef.current?.controller.stop();
        renewalRef.current = null;
        await remoteForceCutoff(activeSshEndpoint.id, "key_revoked");
        useRemoteCutoffStore
          .getState()
          .recordCutoff(activeSshEndpoint.id, "key_revoked");
      }
    } catch (error) {
      setProvisioningError(
        error instanceof Error ? error.message : String(error),
      );
    }
  };

  const handleRegisterUserManaged = async (values: UserManagedFormValues) => {
    setProvisioningError(undefined);
    // Pin exactly the values the user confirmed; alias autofill happens in the form.
    const record: UserManagedEndpointRecord = {
      ...values,
      id: `user-managed-${Date.now()}`,
      created_at: new Date().toISOString(),
    };
    await saveUserManagedEndpoint(record);
    const endpoint = sshEndpointFromUserManaged(record);
    setActiveSshEndpoint(endpoint);
    setActiveEndpointGeneration(0);
    setExplicitEndpointRepoConnected(false);
    setShowRemoteSetupDialog(false);
    await refreshSavedRemoteRepos(endpoint.id, 0);
  };

  const handleOpenManagedRepositories = () => {
    const serverEndpoint = instanceStatus?.endpoint;
    if (!serverEndpoint) return;
    // The control plane's endpoint names its own key id, which the native
    // transport cannot authenticate with. Reuse the active endpoint when it
    // already carries this device's key and certificate; otherwise run the
    // full Connect flow to get them.
    const endpoint =
      activeSshEndpoint?.id === serverEndpoint.id &&
      activeSshEndpoint.authentication.type === "certificate" &&
      activeSshEndpoint.authentication.certificate
        ? activeSshEndpoint
        : null;
    if (!endpoint) {
      void handleConnectManaged();
      return;
    }
    const generation = generationFromEndpoint(
      endpoint,
      instanceStatus?.instance?.generation ?? 0,
    );
    setActiveSshEndpoint(endpoint);
    setActiveEndpointGeneration(generation);
    setExplicitEndpointRepoConnected(false);
    setShowRemoteSetupDialog(false);
    void refreshSavedRemoteRepos(endpoint.id, generation);
  };

  const terminalPaneRef = useRef<WorkspaceTerminalPaneHandle>(null);

  const { addToast } = useToast();
  useAutoUpdate({
    autoCheck: import.meta.env.MODE !== "test",
    listenMenu: import.meta.env.MODE !== "test",
  });
  const handleReturnToDashboard = () => {
    // Navigate to main repo ShowWorkspace > Code
    setSelectedWorkspace(null);
    setActiveSessionId(null);
  };

  // Leave the workspace view only when the workspace on screen was removed.
  const leaveRemovedWorkspaces = (removedIds: ReadonlySet<number>) => {
    // Removal retargets children, changing their commits-ahead counts.
    void invalidateQueries(["workspace-status", queryRepoKey]);
    if (!selectedWorkspace || !removedIds.has(selectedWorkspace.id)) return;
    // Land on the removed workspace's stack parent rather than the home repo.
    const parent = workspaces.find(
      (workspace) =>
        workspace.branch_name === selectedWorkspace.target_branch &&
        !removedIds.has(workspace.id),
    );
    if (parent) {
      setSelectedWorkspace(parent);
      setActiveSessionId(null);
    } else {
      handleReturnToDashboard();
    }
  };

  const openSettings = (tab?: string) => {
    void tab;
    if (viewMode !== "settings") {
      previousViewModeRef.current = viewMode;
    }
    setViewMode("settings");
  };

  const closeSettings = () => {
    setViewMode(previousViewModeRef.current);
  };

  const openArtifacts = () => {
    if (viewMode !== "artifacts") {
      previousViewModeRef.current = viewMode;
    }
    setViewMode("artifacts");
    navigate(artifactsPath());
  };

  const closeArtifacts = () => {
    setViewMode(previousViewModeRef.current);
    navigate("/", { replace: true });
  };

  const openGitHub = () => {
    if (viewMode !== "github") {
      previousViewModeRef.current = viewMode;
    }
    setViewMode("github");
    navigate(githubListPath("issues"));
  };

  const openLinear = () => {
    if (viewMode !== "linear") {
      previousViewModeRef.current = viewMode;
    }
    setViewMode("linear");
    navigate(LINEAR_BASE_PATH);
  };

  const openTracker = (provider: TrackerProvider) => {
    if (viewMode !== provider) {
      previousViewModeRef.current = viewMode;
    }
    setViewMode(provider);
    navigate(TRACKER_PROVIDERS[provider].basePath);
  };

  const openGitHubPr = (prNumber: number, prState: string) => {
    if (viewMode !== "github") {
      previousViewModeRef.current = viewMode;
    }
    setViewMode("github");
    navigate(githubDetailPath("prs", prNumber, stateFilterForPrState(prState)));
  };

  // Browser back/forward can pop the URL out of the GitHub section (e.g. the
  // user opened it via the sidebar, then hit Back) without going through
  // openGitHub/openSettings, so mirror that into viewMode here.
  useEffect(() => {
    if (viewMode === "github" && !location.startsWith(GITHUB_BASE_PATH)) {
      setViewMode(previousViewModeRef.current);
    }
    if (viewMode === "artifacts" && !location.startsWith(ARTIFACTS_BASE_PATH)) {
      setViewMode(previousViewModeRef.current);
    }
    if (viewMode === "linear" && !location.startsWith(LINEAR_BASE_PATH)) {
      setViewMode(previousViewModeRef.current);
    }
    if (location.startsWith(ARTIFACTS_BASE_PATH) && viewMode !== "artifacts") {
      if (viewMode !== "github") {
        previousViewModeRef.current = viewMode;
      }
      setViewMode("artifacts");
    }
    if (location.startsWith(LINEAR_BASE_PATH) && viewMode !== "linear") {
      if (viewMode !== "github" && viewMode !== "artifacts") {
        previousViewModeRef.current = viewMode;
      }
      setViewMode("linear");
    }
    // A disabled tracker's route falls back to the previous view, so a stale
    // URL or turning the preview off mid-view never leaves a blank pane.
    const trackerRouteEnabled: Record<TrackerProvider, boolean> = {
      trello: trelloIntegrationEnabled,
      jira: jiraIntegrationEnabled,
    };
    for (const { id, basePath } of Object.values(TRACKER_PROVIDERS)) {
      const enabled = trackerRouteEnabled[id];
      if (viewMode === id && (!enabled || !location.startsWith(basePath))) {
        setViewMode(previousViewModeRef.current);
      }
      if (enabled && location.startsWith(basePath) && viewMode !== id) {
        if (viewMode !== "github" && viewMode !== "artifacts") {
          previousViewModeRef.current = viewMode;
        }
        setViewMode(id);
      }
    }
  }, [location, viewMode, trelloIntegrationEnabled, jiraIntegrationEnabled]);

  // The reverse also happens: leaving "github" through a non-URL action (e.g.
  // clicking a workspace in the sidebar) should clear the now-stale
  // /github URL so Back/Forward doesn't get stuck pointing at a view that's
  // no longer showing.
  const previousViewModeForUrlRef = useRef<ViewMode>(viewMode);
  useEffect(() => {
    const leftGitHub =
      previousViewModeForUrlRef.current === "github" && viewMode !== "github";
    const leftArtifacts =
      previousViewModeForUrlRef.current === "artifacts" &&
      viewMode !== "artifacts";
    const leftLinear =
      previousViewModeForUrlRef.current === "linear" && viewMode !== "linear";
    const leftTracker = Object.values(TRACKER_PROVIDERS).find(
      ({ id }) => previousViewModeForUrlRef.current === id && viewMode !== id,
    );
    previousViewModeForUrlRef.current = viewMode;
    if (leftTracker && location.startsWith(leftTracker.basePath)) {
      navigate("/", { replace: true });
    }
    if (leftGitHub && location.startsWith(GITHUB_BASE_PATH)) {
      navigate("/", { replace: true });
    }
    if (leftArtifacts && location.startsWith(ARTIFACTS_BASE_PATH)) {
      navigate("/", { replace: true });
    }
    if (leftLinear && location.startsWith(LINEAR_BASE_PATH)) {
      navigate("/", { replace: true });
    }
  }, [viewMode, location, navigate]);

  const handleOpenMergePreview = () => {
    if (!remoteCaps.mergeWorkspace.supported) {
      addToast({
        title: "Merge unavailable",
        description: remoteCaps.mergeWorkspace.reason,
        type: "warning",
      });
      return;
    }
    if (selectedWorkspace) {
      setMergeWorkspace(selectedWorkspace);
      setViewMode("merge-preview");
    }
  };

  const { data: repoBranch } = useSWR(
    queryRepoKey ? ["repo-branch", queryRepoKey] : null,
    () => getRepoCurrentBranch(dataRepoPath),
  );
  const { data: repoDefaultBranch } = useSWR(
    queryRepoKey ? ["repo-default-branch", queryRepoKey] : null,
    () => getRepoDefaultBranch(dataRepoPath),
  );

  useEffect(() => {
    if (!dataRepoPath) return;
    if (!repoBranch) return;
    setCurrentBranch(repoBranch.current_branch);
    setHomeRepoDisplayRef(repoBranch.display_ref);
  }, [dataRepoPath, repoBranch]);

  const effectiveDefaultBranch = currentBranch || repoDefaultBranch || "main";

  // Stack from the home repo parents the new workspace on the home branch.
  // Until that branch has loaded, effectiveDefaultBranch is only the "main"
  // fallback, and stacking on it registered a workspace for "main". Fetch
  // the branch instead of guessing.
  const resolveHomeStackBranch = async (): Promise<string | null> => {
    try {
      const branch = repoBranch
        ? (currentBranch ?? repoBranch.current_branch)
        : (await getRepoCurrentBranch(dataRepoPath)).current_branch;
      return (
        branch ||
        repoDefaultBranch ||
        (await getRepoDefaultBranch(dataRepoPath))
      );
    } catch {
      return null;
    }
  };

  const openHomeStackDialog = async () => {
    if (!dataRepoPath) return;
    const targetBranch = await resolveHomeStackBranch();
    if (!targetBranch) {
      addToast({
        title: "Cannot create stacked workspace",
        description: "No parent branch available",
        type: "error",
      });
      return;
    }
    setUnifiedDialogDefaults({ targetBranch, sourceWorkspace: null });
  };

  const handleCreateStackedWorkspace = () => {
    if (!dataRepoPath) return;

    if (selectedWorkspace) {
      setUnifiedDialogDefaults({
        targetBranch: selectedWorkspace.branch_name,
        sourceWorkspace: selectedWorkspace,
      });
    } else {
      void openHomeStackDialog();
    }
  };

  // Keyboard shortcuts
  useKeyboardShortcut("n", true, () => {
    setUnifiedDialogDefaults({});
  });

  useKeyboardShortcut(
    "n",
    true,
    () => {
      handleCreateStackedWorkspace();
    },
    [selectedWorkspace, effectiveDefaultBranch, repoBranch],
    { shift: true },
  );

  useKeyboardShortcut("k", true, () => {
    setShowCommandPalette(true);
  });

  useKeyboardShortcut("i", true, () => {
    setShowAgentPromptDialog(true);
  });

  useKeyboardShortcut("p", true, () => {
    setShowFilePicker(true);
  });

  // "?": show keyboard shortcut reference (Shift+/ on US keyboards)
  useKeyboardShortcut(
    "?",
    false,
    () => {
      setShowKeyboardShortcuts((open) => !open);
    },
    [],
    { ignoreShift: true },
  );

  useKeyboardShortcut(
    "Escape",
    false,
    () => {
      // UnifiedWorkspaceDialog handles its own Escape (it stays open mid-create).
      if (showCommandPalette) setShowCommandPalette(false);
      if (showFilePicker) setShowFilePicker(false);
      setShowKeyboardShortcuts(false);
    },
    [showCommandPalette, showFilePicker],
  );

  useSWR(
    repoPath && !isRemoteActive ? ["init-repo", repoPath] : null,
    async () => {
      try {
        await initRepo(repoPath);
      } catch (e) {
        console.error("Failed to init repo:", e);
      }
      await setWindowRepoPath(repoPath);
      return true;
    },
  );

  useEffect(() => {
    if (!dataRepoPath) {
      setCurrentBranch(null);
      setHomeRepoDisplayRef(null);
    }
  }, [dataRepoPath]);

  const {
    data: availableBranches = [],
    isValidating: branchesLoading,
    mutate: loadAvailableBranches,
  } = useSWR<BranchListItem[]>(
    dataRepoPath ? ["repo-branches", queryRepoKey] : null,
    async () => {
      const jjBranches = await listRepoBranches(dataRepoPath);
      return jjBranches.map((branch) => ({
        name: branch.name,
        fullName: branch.name,
        isCurrent: branch.is_current,
      }));
    },
    {
      revalidateOnMount: false,
      revalidateIfStale: false,
      revalidateOnFocus: false,
      revalidateOnReconnect: false,
      dedupingInterval: 5 * 60 * 1000,
    },
  );

  const handleLoadAvailableBranches = () => {
    if (!dataRepoPath) return;
    void loadAvailableBranches();
  };

  // Manage file watcher lifecycle for selected workspace
  // useEffect(() => {
  // 	if (viewMode !== "show-workspace") return;
  // 	if (showWorkspaceActiveTab !== "changes") return;
  // 	if (!selectedWorkspace) return;

  // 	const workspaceId = selectedWorkspace.id;
  // 	const workspacePath = getFullWorkspacePath(selectedWorkspace);

  // 	// startFileWatcher(workspaceId, workspacePath).catch((err) => {
  // 	// 	console.error("Failed to start file watcher:", err);
  // 	// });

  // 	// // Stop watching when workspace changes or component unmounts
  // 	// return () => {
  // 	// 	stopFileWatcher(workspaceId, workspacePath).catch((err) => {
  // 	// 		console.error("Failed to stop file watcher:", err);
  // 	// 	});
  // 	// };
  // }, [
  // 	viewMode,
  // 	showWorkspaceActiveTab,
  // 	selectedWorkspace?.id,
  // 	selectedWorkspace?.repo_path,
  // 	selectedWorkspace?.workspace_path,
  // ]);

  const { data: sessions = [] } = useSWR(
    queryRepoKey ? ["sessions", queryRepoKey] : null,
    () => getSessions(dataRepoPath).catch(() => []),
    { refreshInterval: pollMs(30000) },
  );

  const { data: workspaces = [] } = useSWR(
    queryRepoKey ? ["workspaces", queryRepoKey] : null,
    () => getWorkspaces(dataRepoPath),
    { refreshInterval: pollMs(10000) },
  );

  // `workspaces` is refetched (e.g. after a push, or on its 10s interval) and
  // returns fresh object references every time, but `selectedWorkspace` is a
  // point-in-time snapshot. Without this, fields like `not_on_remote` on the
  // currently open workspace go stale until the user re-selects it (e.g. the
  // "Push to remote" button would keep showing after a successful push).
  // Compare by value, not reference, so this only re-renders when the
  // workspace's data actually changed.
  useEffect(() => {
    setSelectedWorkspace((current) => {
      if (!current) return current;
      const updated = workspaces.find((w) => w.id === current.id);
      if (!updated || JSON.stringify(updated) === JSON.stringify(current)) {
        return current;
      }
      return updated;
    });
  }, [workspaces]);

  // A workspace removed outside this view (CLI, another window) must not stay
  // open. Only react to ids that were listed before, so a workspace selected
  // right after creation is not dropped while the list catches up.
  const listedWorkspaceIdsRef = useRef<ReadonlySet<number>>(new Set());
  useEffect(() => {
    const previous = listedWorkspaceIdsRef.current;
    const current = new Set(workspaces.map((w) => w.id));
    listedWorkspaceIdsRef.current = current;
    if (
      selectedWorkspace &&
      previous.has(selectedWorkspace.id) &&
      !current.has(selectedWorkspace.id)
    ) {
      handleReturnToDashboard();
    }
  }, [workspaces]);

  const { data: workspaceStatuses = [] } = useSWR(
    queryRepoKey ? ["workspace-statuses", queryRepoKey] : null,
    () => listWorkspaceStatuses(dataRepoPath),
    { refreshInterval: pollMs(10000) },
  );

  const visibleWorkspaces = (() => {
    if (workspaceStatuses.length === 0) {
      return workspaces;
    }
    return flattenWorkspaceTree(buildWorkspaceTree(workspaceStatuses)).map(
      (node) => node.status.current,
    );
  })();

  // Note: Git cache preloader removed since we're using JJ now

  const deleteWorkspaceMutation = useMutation({
    mutationFn: async (workspace: Workspace) => {
      await deleteWorkspace(workspace.repo_path, workspace.id);
    },
    onSuccess: (_data, workspace) => {
      terminalPaneRef.current?.closeTerminalsForWorkspace(
        getFullWorkspacePath(workspace),
      );
      void invalidateQueries(["workspaces", queryRepoKey]);
      invalidateQueries(["workspace-statuses", queryRepoKey]);
      leaveRemovedWorkspaces(new Set([workspace.id]));
      addToast({
        title: "Workspace Deleted",
        description: "Workspace has been deleted",
        type: "success",
      });
    },
    onError: (error) => {
      addToast({
        title: "Delete Failed",
        description: error instanceof Error ? error.message : String(error),
        type: "error",
      });
    },
  });

  const archiveWorkspaceMutation = useMutation({
    mutationFn: async (workspace: Workspace) => {
      await archiveWorkspace(workspace.repo_path, workspace.id);
    },
    onSuccess: (_data, workspace) => {
      terminalPaneRef.current?.closeTerminalsForWorkspace(
        getFullWorkspacePath(workspace),
      );
      void invalidateQueries(["workspaces", queryRepoKey]);
      invalidateQueries(["workspace-statuses", queryRepoKey]);
      leaveRemovedWorkspaces(new Set([workspace.id]));
      addToast({
        title: "Workspace Archived",
        description: "Workspace directory removed; record kept",
        type: "success",
      });
    },
    onError: (error) => {
      addToast({
        title: "Archive Failed",
        description: error instanceof Error ? error.message : String(error),
        type: "error",
      });
    },
  });

  const openRepositoryAtPath = (selected: string) =>
    openRepositoryAtPathShared(selected, {
      addToast,
      onOpened: (path) => {
        setRepoPath(path);
        setSelectedWorkspace(null);
        setActiveSessionId(null);
        setSessionSelectedFile(null);
      },
    });

  const handleOpenRepository = async () => {
    const selected = await selectFolder();
    if (!selected) return;
    await openRepositoryAtPath(selected);
  };

  const handleOpenRepositoryInNewWindow = async () => {
    const selected = await selectFolder();
    if (!selected) return;
    const windowLabel = `treq-repo-${Date.now()}-${Math.floor(
      Math.random() * 1000,
    )}`;
    const repoName =
      selected.split("/").pop() || selected.split("\\").pop() || selected;
    new WebviewWindow(windowLabel, {
      url: `index.html?repo=${encodeURIComponent(selected)}`,
      title: `Treq - ${repoName}`,
      width: 1400,
      height: 900,
    });
  };

  // Restore the last-opened remote SSH repository only after reconnect and
  // trust validation. A stored blob or descriptor is not enough on its own.
  useEffect(() => {
    if (!remoteSshEnabled || repoPath) {
      return;
    }
    let cancelled = false;
    void (async () => {
      const id = await getSetting(LAST_OPENED_REMOTE_REPO_ID_KEY).catch(
        () => null,
      );
      // Without a saved descriptor there is no endpoint to validate trust
      // against; the legacy endpoint-less blob is never restored on its own.
      if (!id || cancelled) return;
      const descriptor = await getSavedRemoteRepository(id);
      if (!descriptor || cancelled) return;

      const userManaged = (await listUserManagedEndpoints()).find(
        (endpoint) => endpoint.id === descriptor.endpoint_id,
      );
      let endpoint: SshEndpoint | null = userManaged
        ? sshEndpointFromUserManaged(userManaged)
        : null;
      let generation = descriptor.endpoint_generation;
      if (!endpoint) {
        const status = await getInstanceStatus().catch(() => null);
        const { endpoint: statusEndpoint, instance } = status ?? {};
        if (statusEndpoint?.id === descriptor.endpoint_id) {
          endpoint = statusEndpoint;
          generation = generationFromEndpoint(
            statusEndpoint,
            instance?.generation ?? descriptor.endpoint_generation,
          );
          if (!cancelled && status) setInstanceStatus(status);
        }
      }
      if (!endpoint || cancelled) return;
      const activeEndpoint = endpoint;

      const result = await restoreSavedRemoteRepository({
        descriptor,
        currentGeneration: generation,
        explicitGenerationTransition: false,
        reconnect: async () => {
          try {
            await dispatchOverSsh(activeEndpoint, {
              kind: "ProbeRepo",
              repo: descriptor.canonical_remote_path,
            });
            return true;
          } catch {
            return false;
          }
        },
        validateHostKey: async () =>
          activeEndpoint.host_keys.some(
            (key) => key.fingerprint_sha256.length > 0,
          ),
        inspect: (canonicalPath) =>
          dispatchOverSsh<RepositoryInspection>(activeEndpoint, {
            kind: "InspectRepository",
            repo: canonicalPath,
          }),
      });
      if (cancelled) return;
      if (!result.ok) {
        setActiveRemoteRepo(null);
        setExplicitEndpointRepoConnected(false);
        return;
      }
      setActiveSshEndpoint(activeEndpoint);
      setActiveEndpointGeneration(result.descriptor.endpoint_generation);
      setSelectedSavedRepoId(result.descriptor.id);
      setExplicitEndpointRepoPath(result.descriptor.canonical_remote_path);
      setExplicitEndpointRepoConnected(true);
      await refreshSavedRemoteRepos(
        result.descriptor.endpoint_id,
        result.descriptor.endpoint_generation,
      );
      const connectedRepo = connectedRepoFromDescriptor(
        result.descriptor,
        activeEndpoint,
        result.inspection,
      );
      if (!cancelled) setActiveRemoteRepo(connectedRepo);
    })();
    return () => {
      cancelled = true;
    };
  }, [repoPath, remoteSshEnabled]);

  // Consolidate all Tauri event listeners
  useEffect(() => {
    const listeners = [
      // Git config init error handler
      // listen<{ repo_path: string; error: string }>(
      //   "git-config-init-error",
      //   (event) => {
      //     const { repo_path, error } = event.payload;
      //     if (repoPath && repo_path === repoPath) {
      //       addToast({
      //         title: "Git configuration warning",
      //         description: `Could not configure automatic remote tracking: ${error}`,
      //         type: "warning",
      //       });
      //     }
      //   }
      // ),
      // Navigate to settings
      listen("navigate-to-settings", () => {
        openSettings();
      }),
      // Menu open repository - path is resolved natively before this fires
      listen<string>("menu-repository-path-selected", (event) => {
        void openRepositoryAtPath(event.payload);
      }),
      // Menu open via SSH
      listen("menu-open-ssh", () => {
        if (!useFeaturePreviewStore.getState().flags.remoteSsh) return;
        void handleOpenRemoteSetup();
      }),
      // Menu factory reset
      listen("menu-factory-reset", async () => {
        const confirmed = await ask(
          "Clear the saved repository path and return to the start screen?",
          {
            title: "Factory Reset",
            kind: "warning",
          },
        );
        if (!confirmed) return;

        await setSetting("last_opened_repo_path", "");
        await setWindowRepoPath("");
        setRepoPath("");
        setSelectedWorkspace(null);
        setActiveSessionId(null);
        setSessionSelectedFile(null);
        void clearSWRCache();

        addToast({
          title: "Factory Reset Complete",
          description: "Repository path cleared.",
          type: "success",
        });
      }),
      listen("menu-open-in-new-window-invalid", () => {
        addToast({
          title: "Not a Git Repository",
          description: "Please select a folder that contains a git repository.",
          type: "error",
        });
      }),
      // Developer menu force rebase for current workspace
      listen("menu-force-rebase-workspace", async () => {
        if (!repoPath || !selectedWorkspace) {
          addToast({
            title: "No workspace selected",
            description: "Select a workspace first, then force rebase.",
            type: "warning",
          });
          return;
        }

        const defaultBranch =
          selectedWorkspace.target_branch || effectiveDefaultBranch;
        try {
          const result = await checkAndRebaseWorkspaces(
            repoPath,
            selectedWorkspace.id,
            defaultBranch,
            true,
          );

          void invalidateQueries(["workspaces", queryRepoKey]);
          invalidateQueries(["workspace-statuses", queryRepoKey]);

          addToast({
            title: result.success
              ? "Force rebase complete"
              : "Force rebase completed with errors",
            description:
              result.message || "Workspace subtree force rebase finished.",
            type: result.success ? "success" : "warning",
          });
        } catch (error) {
          addToast({
            title: "Force rebase failed",
            description: error instanceof Error ? error.message : String(error),
            type: "error",
          });
        }
      }),
    ];

    return () => {
      Promise.all(listeners).then((unlistenFns) => {
        unlistenFns.forEach((fn) => fn());
      });
    };
  }, [
    repoPath,
    selectedWorkspace,
    effectiveDefaultBranch,
    addToast,
    deleteWorkspaceMutation,
    handleOpenRepository,
    openSettings,
  ]);

  // Note: Git merge functionality removed - using JJ now

  // Helper to create or get session
  const getOrCreateSession = async (
    workspaceId: number | null,
    options?: {
      forceNew?: boolean;
      name?: string;
      agent?: AgentKind;
    },
  ): Promise<number> => {
    const sessions = await getSessions(repoPath);
    if (!options?.forceNew) {
      const existing = sessions.find((s) => s.workspace_id === workspaceId);
      if (existing) {
        await updateSessionAccess(repoPath, existing.id);
        return existing.id;
      }
    }

    const scopedSessions = sessions.filter(
      (s) => s.workspace_id === workspaceId,
    );
    const index = scopedSessions.length + 1;
    let name = options?.name;
    if (!name) {
      name = `${agentInfo(options?.agent ?? DEFAULT_AGENT).label} ${index}`;
    }

    const sessionId = await createSession(repoPath, workspaceId, name);

    void invalidateQueries(["sessions"]);
    return sessionId;
  };

  const handleOpenSession = async (
    workspace: Workspace | null,
    options?: SessionOpenOptions,
  ) => {
    setSelectedWorkspace(workspace);
    setSessionSelectedFile(options?.selectedFilePath ?? null);
  };

  // A session started away from the visible terminal (a tracker kickoff, or a
  // dialog pointed at another workspace) gets a toast instead of a jump.
  const announceSessionStarted = async (
    sessionId: number,
    workspaceId: number | null,
  ) => {
    const known =
      workspaceId == null || workspaces.some((ws) => ws.id === workspaceId)
        ? workspaces
        : await fetchAndCache(["workspaces", queryRepoKey], () =>
            getWorkspaces(dataRepoPath),
          ).catch(() => workspaces);
    const workspace = known.find((ws) => ws.id === workspaceId) ?? null;
    addToast({
      title: "Session started",
      description: `in ${workspace?.branch_name ?? effectiveDefaultBranch}`,
      type: "success",
      action: {
        label: "Open",
        onClick: () => {
          handleSelectWorkspace(workspace);
          setActiveSessionId(sessionId);
        },
      },
    });
  };

  const handleSessionCreated = (sessionData: {
    sessionId: number;
    workspaceId?: number | null;
    workspacePath?: string | null;
    pendingPrompt?: string;
    permissionMode?: "plan" | "acceptEdits";
    agent?: AgentKind;
  }) => {
    void invalidateQueries(["sessions"]);
    // The session may have created its own workspace (a Linear kickoff does),
    // so refresh the list when it points at a workspace we don't know yet.
    if (
      sessionData.workspaceId != null &&
      !workspaces.some((ws) => ws.id === sessionData.workspaceId)
    ) {
      void invalidateQueries(["workspaces", queryRepoKey]);
      invalidateQueries(["workspace-statuses", queryRepoKey]);
    }
    setActiveSessionId(sessionData.sessionId);
    const targetWorkspaceId = sessionData.workspaceId ?? null;
    const onScreen =
      isSessionView && (selectedWorkspace?.id ?? null) === targetWorkspaceId;
    if (!onScreen) {
      void announceSessionStarted(sessionData.sessionId, targetWorkspaceId);
    }
    if (
      sessionData.pendingPrompt ||
      sessionData.permissionMode ||
      sessionData.agent ||
      sessionData.workspacePath
    ) {
      setPendingSessionData((prev) => {
        const next = new Map(prev);
        next.set(sessionData.sessionId, {
          pendingPrompt: sessionData.pendingPrompt,
          permissionMode: sessionData.permissionMode,
          agent: sessionData.agent,
          workspacePath: sessionData.workspacePath,
        });
        return next;
      });
    }
    if (sessionData.pendingPrompt) {
      addPromptHistory(
        repoPath,
        sessionData.workspaceId ?? null,
        sessionData.sessionId,
        sessionData.pendingPrompt,
        sessionData.agent,
      )
        .then(() => {
          void invalidateQueries(["prompt-history"]);
          invalidateQueries(["workspace-starting-prompt"]);
        })
        .catch((error) => {
          console.error("Failed to record prompt history:", error);
        });
    }
  };

  const handleViewFullPrompt = (promptId: number) => {
    setPromptHistoryFocusId(promptId);
    setShowPromptHistory(true);
  };

  const handlePromptHistoryOpenChange = (open: boolean) => {
    setShowPromptHistory(open);
    if (!open) setPromptHistoryFocusId(null);
  };

  const handleRunPrompt = (prompt: string, workspaceId: number | null) => {
    setShowPromptHistory(false);
    setPromptHistoryFocusId(null);
    setRunPromptRequest({ prompt, workspaceId });
    setShowAgentPromptDialog(true);
  };

  // Every tracker panel starts a prompt the same way: attach the issue and
  // open the shared prompt dialog.
  const handleStartPromptFromIssue = (issue: IssueAttachment) => {
    setRunPromptRequest({ workspaceId: null, issue });
    setShowAgentPromptDialog(true);
  };

  const handleAgentPromptDialogOpenChange = (open: boolean) => {
    setShowAgentPromptDialog(open);
    if (!open) setRunPromptRequest(null);
  };

  const handleStartDefaultAgent = async () => {
    const configuredAgent =
      (await getRepoSetting(repoPath, "default_agent")) ||
      (await getSetting("default_agent"));
    const agent = toAgentKind(configuredAgent) ?? DEFAULT_AGENT;
    const sessionId = await getOrCreateSession(selectedWorkspace?.id ?? null, {
      forceNew: true,
      agent,
    });
    handleSessionCreated({
      sessionId,
      workspaceId: selectedWorkspace?.id ?? null,
      agent,
    });
  };

  // Navigate to workspace without creating an agent session
  const handleSelectWorkspace = (workspace: Workspace | null) => {
    const next = workspace ?? null;
    setSelectedWorkspace(next);
    setViewMode("show-workspace");
    if (repoPath) {
      void invalidateReviewChangeCount(queryRepoKey, next?.id ?? null);
    }
  };

  const { moveWorkspace } = useWorkspaceHierarchy({
    repoPath: dataRepoPath,
    workspaces,
    defaultBranch: effectiveDefaultBranch,
  });

  const handleAddAfter = (workspace: Workspace) => {
    setUnifiedDialogDefaults({
      targetBranch: workspace.branch_name,
      sourceWorkspace: workspace,
    });
  };

  const handleAddBefore = (workspace: Workspace) => {
    setUnifiedDialogDefaults({
      targetBranch: workspace.branch_name,
      sourceWorkspace: workspace,
    });
  };

  const handleMoveWorkspace = async (
    workspace: Workspace,
    targetBranch: string | null,
  ) => {
    try {
      await moveWorkspace(workspace, targetBranch);
    } catch (error) {
      addToast({
        title: "Failed to move workspace",
        description: error instanceof Error ? error.message : String(error),
        type: "error",
      });
    }
  };

  const handleDropChangeFiles = (request: ChangeFilesMoveRequest) => {
    if (
      request.files.length === 0 ||
      request.sourceBranch === request.destinationBranch
    ) {
      return;
    }
    setPendingChangeMove(request);
  };

  const handleConfirmChangeMove = async () => {
    if (!pendingChangeMove || !repoPath) return;
    setChangeMovePending(true);
    try {
      await moveWorkspaceChanges(
        repoPath,
        pendingChangeMove.sourceBranch,
        pendingChangeMove.destinationBranch,
        {
          files: pendingChangeMove.files,
          hunks: [],
          commits: [],
        },
      );
      addToast({
        title: "Files moved",
        description: `Moved ${pendingChangeMove.files.length} file(s) to ${pendingChangeMove.destinationLabel}`,
        type: "success",
      });
      setPendingChangeMove(null);
      await invalidateQueries(["workspaces", queryRepoKey]);
      await invalidateQueries(["workspace-statuses", queryRepoKey]);
      await invalidateReviewChangeCount(
        repoPath,
        selectedWorkspace?.id ?? null,
      );
      dispatchRefreshWorkspaceChanges();
    } catch (error) {
      addToast({
        title: "Failed to move files",
        description: error instanceof Error ? error.message : String(error),
        type: "error",
      });
    } finally {
      setChangeMovePending(false);
    }
  };

  const handleSelectStack = (workspaceIds: Set<number>) => {
    setSelectedWorkspaceIds(workspaceIds);
  };

  /**
   * Where a remote shell or agent for `workspace` runs: the endpoint, the
   * repository and workspace that scope its persistent session name, and
   * the directory on the host it starts in. Returns `null` (after telling
   * the user why) when no remote session can be opened right now.
   */
  const remoteSessionScope = (
    workspace: Workspace | null,
  ): Omit<RemoteTerminalTarget, "label" | "launch" | "reattach"> | null => {
    const repositoryId = activeRepository?.canonicalPath;
    if (!isRemoteActive || !repositoryId) return null;
    if (cutoffReason || !activeSshEndpoint) {
      addToast({
        title: "Remote terminal unavailable",
        description: cutoffReason
          ? "Reconnect to the remote host before opening a terminal."
          : "Connect to this repository's SSH host to open a terminal.",
        type: "warning",
      });
      return null;
    }
    return {
      endpoint: activeSshEndpoint,
      repositoryId,
      ...resolveRemoteTerminalTarget(
        repositoryId,
        workspace
          ? {
              workspace_name: workspace.workspace_name,
              workspace_path: getFullWorkspacePath(workspace),
            }
          : null,
      ),
    };
  };

  const openNewRemoteSession = (
    workspace: Workspace | null,
    agent: RemoteAgentId | null,
  ) => {
    const scope = remoteSessionScope(workspace);
    if (!scope) return;
    terminalPaneRef.current?.openRemoteSession({
      ...scope,
      label: newRemoteSessionLabel(agent),
      launch: remoteLaunchFor(agent),
      reattach: false,
    });
  };

  const resolveRemoteShell = (
    workingDirectory: string,
  ): RemoteTerminalTarget | null => {
    const workspace =
      workspaces.find((w) => getFullWorkspacePath(w) === workingDirectory) ??
      null;
    const scope = remoteSessionScope(workspace);
    if (!scope) return null;
    return {
      ...scope,
      label: newRemoteSessionLabel(null),
      launch: remoteLaunchFor(null),
      reattach: false,
    };
  };

  /**
   * Remote counterpart of the local agent flow below. The agent runs inside
   * a persistent session on the host, in the workspace's directory, so it
   * keeps running when the pane closes or the connection drops. Only the
   * app-level default agent applies: repository settings are stored in the
   * local checkout, which a remote repository does not have.
   */
  const startRemoteAgent = async (
    workspace: Workspace | null,
    agent?: "claude" | "codex" | "cursor" | "copilot",
  ) => {
    let chosen = agent;
    if (!chosen) {
      const appDefault = await getSetting("default_agent").catch(() => null);
      chosen =
        appDefault === "codex" ||
        appDefault === "cursor" ||
        appDefault === "copilot"
          ? appDefault
          : "claude";
    }
    const remoteAgent = remoteAgentIdFor(chosen);
    if (!remoteAgent) {
      addToast({
        title: "Agent unavailable on remote hosts",
        description:
          "Copilot cannot be started in a remote workspace yet. Choose Claude, Codex or Cursor.",
        type: "warning",
      });
      return;
    }
    setSelectedWorkspace(workspace);
    openNewRemoteSession(workspace, remoteAgent);
  };

  const handleCreateSessionFromSidebar = async (
    workspaceId: number | null,
    agent?: AgentKind,
  ) => {
    if (!remoteCaps.agentPty.supported) {
      addToast({
        title: "Agent terminal unavailable",
        description: remoteCaps.agentPty.reason,
        type: "warning",
      });
      return;
    }
    const workspace = workspaceId
      ? (workspaces.find((w) => w.id === workspaceId) ?? null)
      : null;
    if (isRemoteActive) {
      await startRemoteAgent(workspace, agent);
      return;
    }

    // When no agent is specified explicitly, resolve from settings (repo-level
    // overrides app-level, both fall back to "claude").
    const resolvedAgent = await (async (): Promise<AgentKind | undefined> => {
      if (agent) return agent;
      let repoDefault: string | null = null;
      let appDefault: string | null = null;
      try {
        repoDefault = await getRepoSetting(repoPath, "default_agent");
      } catch {
        // repo may not be initialized yet; fall through to app-level
      }
      try {
        appDefault = await getSetting("default_agent");
      } catch {
        // ignore
      }
      return toAgentKind(repoDefault || appDefault);
    })();

    const sessionId = await getOrCreateSession(workspaceId, {
      forceNew: true,
      agent: resolvedAgent,
    });
    void invalidateQueries(["sessions"]);
    setActiveSessionId(sessionId);
    setSelectedWorkspace(workspace);
    if (resolvedAgent) {
      setPendingSessionData((prev) => {
        const next = new Map(prev);
        next.set(sessionId, { agent: resolvedAgent! });
        return next;
      });
    }
  };

  const remoteConnectionState = cutoffReason
    ? "cutoff"
    : connectionStateFromInstanceState(
        instanceStatus?.instance?.status,
        Boolean(activeSshEndpoint) || Boolean(activeRemoteRepo),
      );

  const handleRefreshRemote = () => {
    if (cutoffReason) return;
    invalidateRemoteRepositoryData();
    void refreshInstanceStatus();
  };

  const handleStartShellFromSidebar = (workspace: Workspace) => {
    if (!remoteCaps.shell.supported) {
      addToast({
        title: "Shell unavailable",
        description: remoteCaps.shell.reason,
        type: "warning",
      });
      return;
    }
    setSelectedWorkspace(workspace);
    terminalPaneRef.current?.createShellSession(
      getFullWorkspacePath(workspace),
    );
  };

  const handleStartHomeShellFromSidebar = () => {
    if (!remoteCaps.shell.supported) {
      addToast({
        title: "Shell unavailable",
        description: remoteCaps.shell.reason,
        type: "warning",
      });
      return;
    }
    setSelectedWorkspace(null);
    if (dataRepoPath) {
      terminalPaneRef.current?.createShellSession(dataRepoPath);
    }
  };

  const handleStackHomeFromSidebar = () => {
    void openHomeStackDialog();
  };

  // Full workspace path -> branch name, used to resolve terminal branches
  // for workspace-level session indicators.
  const workspaceBranchByPath = (() => {
    const map = new Map<string, string>();
    for (const ws of workspaces) {
      map.set(getFullWorkspacePath(ws), ws.branch_name);
    }
    return map;
  })();

  /**
   * Opens an observable agent terminal on a workspace, seeded with a prompt.
   * Shared by agent deep links and automatic reviews so every agent treq
   * starts on its own behalf appears the same way a manually started one does.
   */
  const launchAgentSession = async ({
    workspace,
    prompt,
    mode,
    agent,
    sessionName,
  }: {
    workspace: Workspace;
    prompt?: string;
    mode?: "plan" | "acceptEdits";
    agent?: AgentKind;
    sessionName?: string;
  }): Promise<number> => {
    const sessionId = await getOrCreateSession(workspace.id, {
      forceNew: true,
      agent,
      name: sessionName,
    });
    setActiveSessionId(sessionId);
    setSelectedWorkspace(workspace);
    setViewMode("show-workspace");
    setPendingSessionData((prev) => {
      const next = new Map(prev);
      next.set(sessionId, {
        pendingPrompt: prompt,
        permissionMode: mode,
        agent,
      });
      return next;
    });
    if (prompt) {
      addPromptHistory(repoPath, workspace.id, sessionId, prompt, agent)
        .then(() => {
          void invalidateQueries(["prompt-history"]);
          invalidateQueries(["workspace-starting-prompt"]);
        })
        .catch((error) => {
          console.error("Failed to record prompt history:", error);
        });
    }
    return sessionId;
  };

  const handleStartAgentRequest = async (request: AgentDeepLinkRequest) => {
    const workspace = findWorkspaceByBranch(workspaces, request.branch);
    if (!workspace) {
      addToast({
        title: "Workspace not found",
        description: `Branch '${request.branch}' is not available in this window.`,
        type: "error",
      });
      await acknowledgeAgentDispatch(
        request.requestId,
        "rejected",
        `Workspace branch '${request.branch}' was not found`,
      );
      return;
    }

    try {
      await launchAgentSession({
        workspace,
        prompt: request.prompt,
        mode: request.mode,
        agent: request.agent,
      });
      markProcessedAgentRequest(request.requestId);
      await acknowledgeAgentDispatch(request.requestId, "accepted");
    } catch (error) {
      const reason = error instanceof Error ? error.message : String(error);
      await acknowledgeAgentDispatch(request.requestId, "rejected", reason);
    }
  };
  const handleStartAgentRequestRef = useRef(handleStartAgentRequest);
  handleStartAgentRequestRef.current = handleStartAgentRequest;

  /**
   * Starts a review the backend asked for. The backend has already matched the
   * repository's `auto_review_trigger` against the operation that ran and has
   * dropped operations it fired for before, so every event that arrives here
   * is one to act on.
   */
  const handleAutoReviewEvent = async (event: AutoReviewEvent) => {
    if (event.repo_path !== repoPath) return;
    const workspace = workspaces.find((w) => w.id === event.workspace_id);
    if (!workspace) return;
    try {
      const { prompt, agent } = await prepareWorkspaceReview({
        repoPath,
        workspaceId: event.workspace_id,
        branchName: workspace.branch_name,
        diffSummary: autoReviewSummary(event.trigger, workspace.branch_name),
      });
      await launchAgentSession({
        workspace,
        prompt,
        mode: "acceptEdits",
        agent: toAgentKind(agent),
        sessionName: "AI Review",
      });
    } catch (error) {
      addToast({
        title: "Failed to start automatic review",
        description: error instanceof Error ? error.message : String(error),
        type: "error",
      });
    }
  };
  const handleAutoReviewEventRef = useRef(handleAutoReviewEvent);
  handleAutoReviewEventRef.current = handleAutoReviewEvent;

  useEffect(() => {
    const unlistenPromise = listen<AutoReviewEvent>(
      "auto-review-triggered",
      (event) => {
        void handleAutoReviewEventRef.current(event.payload);
      },
    );
    return () => {
      unlistenPromise.then((fn) => fn());
    };
  }, []);

  useEffect(() => {
    const setup = async () =>
      await listen<string[]>("deep-link-received", async (event) => {
        const requests = parseAgentDeepLinks(event.payload ?? []);
        await processAgentDeepLinkRequests(requests, {
          repoPath,
          workspacesLength: workspaces.length,
          onSameRepoRequest: (request) =>
            handleStartAgentRequestRef.current(request),
          deferRequest: (request) => {
            setDeferredAgentRequests((prev) => [...prev, request]);
          },
          openOtherRepoWindow: (request) => {
            const windowLabel = `treq-agent-${Date.now()}-${Math.floor(
              Math.random() * 1000,
            )}`;
            const newRepoName =
              request.repo.split("/").pop() ||
              request.repo.split("\\").pop() ||
              request.repo;
            new WebviewWindow(windowLabel, {
              url: `index.html?repo=${encodeURIComponent(request.repo)}`,
              title: `Treq - ${newRepoName}`,
              width: 1400,
              height: 900,
            });
          },
        });
      });

    const unlistenPromise = setup();
    return () => {
      unlistenPromise.then((fn) => fn());
    };
  }, [repoPath, workspaces.length]);

  useEffect(() => {
    if (!repoPath || workspaces.length === 0) return;
    const queued = popPendingAgentRequests(repoPath);
    if (queued.length === 0 && deferredAgentRequests.length === 0) return;
    const pending = [...queued, ...deferredAgentRequests];
    setDeferredAgentRequests([]);
    for (const request of pending) {
      if (isProcessedAgentRequest(request.requestId)) continue;
      void handleStartAgentRequestRef.current(request);
    }
  }, [deferredAgentRequests, repoPath, workspaces.length]);

  const handleWorkspaceMultiSelect = (
    workspace: Workspace | null,
    event: React.MouseEvent,
  ) => {
    // Handle clicking away to clear selection
    if (workspace === null) {
      setSelectedWorkspaceIds(new Set());
      lastSelectedWorkspaceIndexRef.current = null;
      return;
    }

    if (archivingWorkspaceIds.size > 0 || exitingWorkspaceIds.size > 0) {
      return;
    }

    const workspaceIndex = visibleWorkspaces.findIndex(
      (w) => w.id === workspace.id,
    );
    if (workspaceIndex === -1) return;

    const isMetaKey = event.metaKey || event.ctrlKey;
    const isShiftKey = event.shiftKey;

    if (isShiftKey && lastSelectedWorkspaceIndexRef.current !== null) {
      // Range selection
      const start = Math.min(
        lastSelectedWorkspaceIndexRef.current,
        workspaceIndex,
      );
      const end = Math.max(
        lastSelectedWorkspaceIndexRef.current,
        workspaceIndex,
      );
      const newSelection = new Set<number>();
      for (let i = start; i <= end; i++) {
        newSelection.add(visibleWorkspaces[i].id);
      }
      setSelectedWorkspaceIds(newSelection);
    } else if (isMetaKey) {
      // Toggle selection
      setSelectedWorkspaceIds((prev) => {
        const next = new Set(prev);
        if (next.has(workspace.id)) {
          next.delete(workspace.id);
        } else {
          next.add(workspace.id);
        }
        return next;
      });
      lastSelectedWorkspaceIndexRef.current = workspaceIndex;
    } else {
      // Regular click - clear multi-select, navigate to workspace
      setSelectedWorkspaceIds(new Set());
      lastSelectedWorkspaceIndexRef.current = workspaceIndex;
      handleSelectWorkspace(workspace);
    }
  };

  const handleBulkArchive = async () => {
    if (archivingWorkspaceIds.size > 0 || exitingWorkspaceIds.size > 0) {
      return;
    }
    const workspacesToArchive = workspaces.filter((w) =>
      selectedWorkspaceIds.has(w.id),
    );
    const ids = new Set(workspacesToArchive.map((w) => w.id));
    setArchivingWorkspaceIds(ids);
    // Settle every archive: a failure must not skip cleanup for the others,
    // which the backend has already archived.
    const results = await Promise.allSettled(
      workspacesToArchive.map((workspace) =>
        archiveWorkspace(workspace.repo_path, workspace.id),
      ),
    );
    const archived = workspacesToArchive.filter(
      (_, index) => results[index].status === "fulfilled",
    );
    const failures = results.flatMap((result) =>
      result.status === "rejected"
        ? [
            result.reason instanceof Error
              ? result.reason.message
              : String(result.reason),
          ]
        : [],
    );
    for (const workspace of archived) {
      terminalPaneRef.current?.closeTerminalsForWorkspace(
        getFullWorkspacePath(workspace),
      );
    }
    const archivedIds = new Set(archived.map((w) => w.id));
    setArchivingWorkspaceIds(new Set());
    setExitingWorkspaceIds(archivedIds);
    if (archived.length > 0) {
      const n = archived.length;
      addToast({
        title: `${n} Workspace${n > 1 ? "s" : ""} Archived`,
        description: `Successfully archived ${n} workspace${n > 1 ? "s" : ""}`,
        type: "success",
      });
    }
    if (failures.length > 0) {
      addToast({
        title: "Archive Failed",
        description: failures.join("\n"),
        type: "error",
      });
    }
    if (archived.length > 0) {
      await new Promise((resolve) => setTimeout(resolve, 220));
      await invalidateQueries(["workspaces", queryRepoKey]);
      invalidateQueries(["workspace-statuses", queryRepoKey]);
      leaveRemovedWorkspaces(archivedIds);
    }
    setSelectedWorkspaceIds(new Set());
    setArchivingWorkspaceIds(new Set());
    setExitingWorkspaceIds(new Set());
  };

  const handleDelete = async (workspace: Workspace) => {
    const confirmed = await ask(`Delete workspace ${workspace.branch_name}?`, {
      title: "Delete Workspace",
      kind: "warning",
    });
    if (confirmed) {
      deleteWorkspaceMutation.mutate(workspace);
    }
  };

  const handleArchive = (workspace: Workspace) => {
    archiveWorkspaceMutation.mutate(workspace);
  };

  // Handle branch change after switching
  const handleBranchChanged = (branchName?: string) => {
    if (branchName) {
      setCurrentBranch(branchName);
      setHomeRepoDisplayRef(branchName);
    }
    void invalidateQueries(["repo-status", queryRepoKey]);
    void invalidateQueries(["repo-branch", queryRepoKey]);
    // Refresh workspace data
    void invalidateQueries(["workspaces", queryRepoKey]);
    invalidateQueries(["workspace-statuses", queryRepoKey]);
    void invalidateQueries(["workspace-review-change-count", repoPath]);
    dispatchRefreshWorkspaceChanges();
  };

  const isSessionView = viewMode === "session" || viewMode === "show-workspace";
  const showSidebar = true;
  const sidebarWidth = useSidebarWidthStore((s) => s.width);
  const sidebarWidthStyle = {
    "--sidebar-width": `${sidebarWidth}px`,
  } as CSSProperties;

  const mainContentStyle = {
    width: showSidebar ? `calc(100vw - ${sidebarWidth}px)` : "100%",
  };

  // Build agent session data for the terminal pane
  const agentSessionsForPane = ((): AgentSessionData[] => {
    const workspaceMap = new Map(workspaces.map((ws) => [ws.id, ws]));

    return sessions.map((session) => {
      const sessionWorkspace = session.workspace_id
        ? (workspaceMap.get(session.workspace_id) ?? null)
        : null;
      const pending = pendingSessionData.get(session.id);
      return {
        sessionId: session.id,
        sessionName: session.name,
        ptySessionId: `session-${session.id}`,
        workspaceId: session.workspace_id,
        workspacePath:
          pending?.workspacePath ??
          (sessionWorkspace ? getFullWorkspacePath(sessionWorkspace) : null),
        repoPath: sessionWorkspace?.repo_path ?? repoPath,
        workspaceName: sessionWorkspace?.branch_name ?? null,
        workingDirectoryOverride: pending?.workspacePath ?? undefined,
        ...(pending && {
          pendingPrompt: pending.pendingPrompt,
          permissionMode: pending.permissionMode,
          agent: pending.agent,
        }),
      };
    });
  })();

  const sessionLayerStyle: CSSProperties = {
    visibility: isSessionView ? "visible" : "hidden",
    zIndex: isSessionView ? 10 : 0,
    pointerEvents: isSessionView ? "auto" : "none",
  };

  const remoteSetupDialog = (
    <RemoteSetupDialog
      open={showRemoteSetupDialog}
      onOpenChange={setShowRemoteSetupDialog}
      sshConfigAliasSuggestions={remoteSshHosts}
      instanceStatus={instanceStatus}
      provisioningStage={provisioningStage}
      provisioningError={provisioningError}
      cloudUsage={cloudUsage}
      cloudUsageError={cloudUsageError}
      onProvisionManaged={handleProvisionManaged}
      onWake={handleWakeManaged}
      onReprovision={handleReprovisionManaged}
      onDeleteInstance={handleDeleteManagedInstance}
      onRevokeKey={handleRevokeKey}
      onRegisterUserManaged={handleRegisterUserManaged}
      onOpenManagedRepositories={handleOpenManagedRepositories}
    />
  );

  const explicitEndpointPathForm =
    activeSshEndpoint && !activeRemoteRepo && !dataRepoPath ? (
      <div className="flex flex-col gap-3 border-b px-4 py-3">
        {cutoffs[activeSshEndpoint.id] && (
          <div className="rounded-md border border-red-500/40 bg-red-500/10 px-3 py-2 text-sm">
            <p className="font-medium text-red-700 dark:text-red-300">
              Access blocked (
              {cutoffs[activeSshEndpoint.id].split("_").join(" ")})
            </p>
            <p className="mt-1 text-muted-foreground">
              This instance is cut off from further commands until you
              reauthenticate and obtain a new certificate.
            </p>
            <Button
              size="sm"
              className="mt-2"
              onClick={() => void handleReauthenticateManaged()}
            >
              Reauthenticate
            </Button>
          </div>
        )}
        {!explicitEndpointRepoConnected && !cutoffs[activeSshEndpoint.id] && (
          <RemoteRepositorySelector
            savedRepositories={savedRemoteRepos}
            selectedId={selectedSavedRepoId}
            path={explicitEndpointRepoPath}
            probe={explicitEndpointProbe}
            cloneUrl={explicitEndpointCloneUrl}
            confirmInit={confirmInitRemoteRepo}
            busy={remoteRepoBusy}
            error={explicitEndpointError}
            onSelectSaved={(id) => void handleSelectSavedRemoteRepo(id)}
            onPathChange={(next) => {
              setExplicitEndpointRepoPath(next);
              setSelectedSavedRepoId(null);
              setExplicitEndpointProbe(null);
              setExplicitEndpointRepoConnected(false);
            }}
            onProbe={() => void handleProbeExplicitEndpointRepo()}
            onCloneUrlChange={setExplicitEndpointCloneUrl}
            onConfirmInitChange={setConfirmInitRemoteRepo}
            onOpenExisting={() => void handleConnectExplicitEndpointRepo()}
            onClone={() => void handleCloneExplicitEndpointRepo()}
            onInit={() => void handleInitExplicitEndpointRepo()}
          />
        )}
      </div>
    ) : null;

  return (
    <ActiveRepositoryProvider repository={activeRepository}>
      {!dataRepoPath ? (
        <>
          <Onboarding
            onOpenRepo={handleOpenRepository}
            onOpenRemoteSsh={
              remoteSshEnabled ? handleOpenRemoteSetup : undefined
            }
          />
          {explicitEndpointPathForm}
          {remoteSetupDialog}
          <RemoteAmbiguousMutationDialog />
        </>
      ) : (
        <SidebarProvider
          className="relative h-screen bg-background"
          style={sidebarWidthStyle}
        >
          {cutoffReason && (
            <div
              data-testid="remote-cutoff-overlay"
              className="absolute inset-0 z-40 bg-background/40"
              aria-hidden
            />
          )}
          <WorkspaceSidebar
            repoPath={dataRepoPath}
            homeRepoDisplayRef={homeRepoDisplayRef}
            selectedWorkspaceId={selectedWorkspace?.id ?? null}
            selectedWorkspaceIds={selectedWorkspaceIds}
            archivingWorkspaceIds={archivingWorkspaceIds}
            exitingWorkspaceIds={exitingWorkspaceIds}
            onWorkspaceClick={(workspace) => handleSelectWorkspace(workspace)}
            onWorkspaceMultiSelect={handleWorkspaceMultiSelect}
            onBulkArchive={handleBulkArchive}
            onArchiveWorkspace={handleArchive}
            openSettings={openSettings}
            navigateToDashboard={handleReturnToDashboard}
            onOpenCommandPalette={() => setShowCommandPalette(true)}
            onOpenBranchSwitcher={() => setShowBranchSwitcher(true)}
            onAddBefore={handleAddBefore}
            onAddAfter={handleAddAfter}
            onMoveWorkspace={handleMoveWorkspace}
            onSelectStack={handleSelectStack}
            onStartAgent={(workspace) =>
              handleCreateSessionFromSidebar(workspace.id)
            }
            onStartShell={handleStartShellFromSidebar}
            onStartHomeAgent={() => handleCreateSessionFromSidebar(null)}
            onStartHomeShell={handleStartHomeShellFromSidebar}
            onStackHome={handleStackHomeFromSidebar}
            terminalSessions={terminalSessionSummaries}
            onDropChangeFiles={handleDropChangeFiles}
            onOpenGitHub={openGitHub}
            onOpenLinear={linearIntegrationEnabled ? openLinear : undefined}
            onOpenTrello={
              trelloIntegrationEnabled ? () => openTracker("trello") : undefined
            }
            onOpenJira={
              jiraIntegrationEnabled ? () => openTracker("jira") : undefined
            }
            onOpenArtifacts={openArtifacts}
            currentPage={
              viewMode === "settings"
                ? "settings"
                : viewMode === "github"
                  ? "github"
                  : viewMode === "artifacts"
                    ? "artifacts"
                    : viewMode === "linear" ||
                        viewMode === "trello" ||
                        viewMode === "jira"
                      ? viewMode
                      : viewMode === "session" || viewMode === "show-workspace"
                        ? "session"
                        : undefined
            }
          />

          <SidebarInset
            className="flex-1 relative min-w-0"
            style={mainContentStyle}
          >
            {/* Sessions Layer - ALWAYS RENDERED ONCE */}
            <div
              className="absolute inset-0 flex flex-col workspace-terminal-container overflow-hidden"
              style={sessionLayerStyle}
            >
              {isRemoteActive && (
                <div className="relative z-50 shrink-0 border-b bg-background">
                  <div className="flex items-center justify-between px-4 py-1 text-xs text-muted-foreground">
                    <span>{activeRepository?.displayName}</span>
                    <div className="flex items-center gap-3">
                      {activeSshEndpoint && (
                        <button
                          type="button"
                          className="underline underline-offset-2"
                          onClick={() => setShowRemoteTerminalDialog(true)}
                          disabled={Boolean(cutoffReason)}
                        >
                          Terminal
                        </button>
                      )}
                      <button
                        type="button"
                        className="underline underline-offset-2"
                        onClick={handleRefreshRemote}
                        disabled={Boolean(cutoffReason)}
                      >
                        Refresh
                      </button>
                    </div>
                  </div>
                  <div className="px-4 pb-1.5">
                    <RemoteStatusBanner
                      state={remoteConnectionState}
                      detail={
                        cutoffReason
                          ? "Access is blocked until you reauthenticate."
                          : undefined
                      }
                      onRefresh={cutoffReason ? undefined : handleRefreshRemote}
                      onWake={() => void handleWakeManaged()}
                      onReconnect={() => void refreshInstanceStatus()}
                    />
                  </div>
                  <RemoteCapabilityNotice capabilities={remoteCaps} />
                </div>
              )}
              {/* Show workspace views take up remaining space */}
              {dataRepoPath && (
                <div className="flex-1 min-h-0 overflow-hidden relative">
                  <ErrorBoundary
                    fallbackTitle="Workspace error"
                    resetKeys={[selectedWorkspace?.id]}
                    onReset={handleReturnToDashboard}
                  >
                    <ShowWorkspace
                      repositoryPath={dataRepoPath}
                      workspace={selectedWorkspace}
                      onActiveTabChange={setShowWorkspaceActiveTab}
                      mainRepoBranch={effectiveDefaultBranch}
                      initialSelectedFile={sessionSelectedFile}
                      availableBranches={availableBranches}
                      branchesLoading={branchesLoading}
                      onLoadAvailableBranches={handleLoadAvailableBranches}
                      onDeleteWorkspace={handleDelete}
                      onOpenFilePicker={() => setShowFilePicker(true)}
                      onOpenMergePreview={handleOpenMergePreview}
                      onViewPrInApp={openGitHubPr}
                      onOpenBranchSwitcher={() => setShowBranchSwitcher(true)}
                      onCreateStackedWorkspace={handleCreateStackedWorkspace}
                      onNavigateToWorkspace={handleSelectWorkspace}
                      onMoveCommitToNewWorkspace={(commit, workspace) => {
                        const firstLine =
                          commit.description.split("\n")[0] || undefined;
                        setUnifiedDialogDefaults({
                          targetBranch: workspace?.branch_name,
                          sourceWorkspace: workspace ?? null,
                          preSelectedCommits: [commit.change_id],
                          description: firstLine,
                          activeTab: "commits",
                        });
                      }}
                      onMoveCommitToExistingWorkspace={(commit, workspace) => {
                        setUnifiedDialogDefaults({
                          targetBranch: workspace?.branch_name,
                          sourceWorkspace: workspace ?? null,
                          preSelectedCommits: [commit.change_id],
                          activeTab: "commits",
                        });
                        // Note: dialog will open in "move to existing" mode via defaults
                      }}
                      onCommitStashed={() => setShowStashModal(true)}
                      onMoveFilesToNewWorkspace={(files, workspace) => {
                        setUnifiedDialogDefaults({
                          targetBranch: workspace?.branch_name,
                          sourceWorkspace: workspace ?? null,
                          preSelectedFiles: files,
                          activeTab: "changes",
                        });
                      }}
                      onSessionCreated={handleSessionCreated}
                      idleAgentSession={terminalSessionSummaries.find(
                        (session) =>
                          session.kind === "agent" &&
                          session.branchName ===
                            selectedWorkspace?.branch_name &&
                          !session.isStreaming,
                      )}
                      onSendToIdleAgent={(sessionId, prompt) => {
                        terminalPaneRef.current?.sendToTerminal(
                          agentTerminalId(sessionId),
                          prompt,
                        );
                      }}
                      onViewFullPrompt={handleViewFullPrompt}
                    />
                  </ErrorBoundary>
                </div>
              )}
              {/* Shared workspace terminal pane - always rendered to preserve state */}
              <WorkspaceTerminalPane
                ref={terminalPaneRef}
                key={queryRepoKey}
                workingDirectory={
                  selectedWorkspace
                    ? getFullWorkspacePath(selectedWorkspace)
                    : dataRepoPath
                }
                currentBranch={effectiveDefaultBranch}
                agentSessions={agentSessionsForPane}
                activeAgentSessionId={isSessionView ? activeSessionId : null}
                workspaceBranchByPath={workspaceBranchByPath}
                onTerminalsChange={setTerminalSessionSummaries}
                resolveRemoteShell={
                  isRemoteActive ? resolveRemoteShell : undefined
                }
                onActiveSessionChange={(sessionId) => {
                  if (sessionId === null) {
                    setActiveSessionId(null);
                    return;
                  }
                  setActiveSessionId(sessionId);
                  // Find the session to determine view mode
                  const session = sessions.find((s) => s.id === sessionId);
                  if (session) {
                    setViewMode(
                      session.workspace_id ? "show-workspace" : "session",
                    );
                    if (session.workspace_id) {
                      const ws = workspaces.find(
                        (w) => w.id === session.workspace_id,
                      );
                      if (ws) setSelectedWorkspace(ws);
                    }
                  }
                }}
                onCloseSession={(sessionId) => {
                  if (activeSessionId === sessionId) {
                    setActiveSessionId(null);
                  }
                }}
                onCreateNewSession={(activeWorkspacePath) => {
                  if (activeWorkspacePath) {
                    const ws = workspaces.find(
                      (w) => getFullWorkspacePath(w) === activeWorkspacePath,
                    );
                    handleCreateSessionFromSidebar(ws?.id ?? null);
                  } else {
                    handleCreateSessionFromSidebar(
                      selectedWorkspace?.id ?? null,
                    );
                  }
                }}
                onNavigateToWorkspace={(workspaceKey, isMainRepo) => {
                  if (isMainRepo) {
                    handleSelectWorkspace(null);
                  } else {
                    const ws = workspaces.find(
                      (w) => w.workspace_path === workspaceKey,
                    );
                    if (ws) {
                      handleSelectWorkspace(ws);
                    }
                  }
                }}
              />
            </div>

            {/* Content Layer - Dashboard, Settings, Merge-Review, Workspace-Edit */}
            <div
              className="absolute inset-0 overflow-auto"
              style={{
                visibility: !isSessionView ? "visible" : "hidden",
                zIndex: !isSessionView ? 10 : 0,
                pointerEvents: !isSessionView ? "auto" : "none",
              }}
            >
              {/* Settings View */}
              {viewMode === "settings" && (
                <SettingsPage
                  repoPath={dataRepoPath}
                  onClose={closeSettings}
                  currentBranch={effectiveDefaultBranch}
                  cloudWorkspace={{
                    instanceStatus,
                    provisioningStage,
                    provisioningError,
                    usage: cloudUsage,
                    usageError: cloudUsageError,
                    onRefreshStatus: refreshCloudWorkspace,
                    onProvision: handleProvisionManaged,
                    onWake: () => handleWakeManaged(),
                    onRepair: handleReprovisionManaged,
                    onDelete: handleDeleteManagedInstance,
                    onOpenRepositories: () => {
                      handleOpenManagedRepositories();
                      closeSettings();
                    },
                  }}
                />
              )}

              {viewMode === "artifacts" && (
                <ArtifactsPage
                  repoPath={dataRepoPath}
                  onClose={closeArtifacts}
                />
              )}

              {/* GitHub Panel */}
              {viewMode === "github" && (
                <GitHubPanel
                  repoPath={dataRepoPath}
                  onOpenSettings={openSettings}
                  onStartPromptFromIssue={(issue) =>
                    handleStartPromptFromIssue(issueFromGitHub(issue))
                  }
                  onOpenWorkspace={async (workspaceId) => {
                    await invalidateQueries(["workspaces", queryRepoKey]);
                    invalidateQueries(["workspace-statuses", queryRepoKey]);
                    const updatedWorkspaces = await fetchAndCache(
                      ["workspaces", queryRepoKey],
                      () => getWorkspaces(dataRepoPath),
                    );
                    const workspace = updatedWorkspaces.find(
                      (w) => w.id === workspaceId,
                    );
                    if (workspace) {
                      handleSelectWorkspace(workspace);
                    }
                  }}
                />
              )}

              {/* Linear Panel */}
              {viewMode === "linear" && linearIntegrationEnabled && (
                <LinearPanel
                  repoPath={dataRepoPath}
                  onStartPromptFromIssue={(issue) =>
                    handleStartPromptFromIssue(issueFromLinear(issue))
                  }
                />
              )}

              {/* Trello / Jira Panels */}
              {(viewMode === "trello" || viewMode === "jira") &&
                trackerEnabled[viewMode] && (
                  <TrackerPanel
                    key={viewMode}
                    provider={viewMode}
                    repoPath={dataRepoPath}
                    onStartPromptFromItem={(item) =>
                      handleStartPromptFromIssue(issueFromTrackerItem(item))
                    }
                  />
                )}

              {/* Merge Preview View */}
              {viewMode === "merge-preview" && mergeWorkspace && (
                <MergePreviewPage
                  workspace={mergeWorkspace}
                  repoPath={dataRepoPath}
                  onCancel={() => {
                    setMergeWorkspace(null);
                    setViewMode("show-workspace");
                  }}
                  onMergeComplete={async () => {
                    // Automatic workspace deletion on merge temporarily disabled
                    // Delete workspace after successful merge
                    // try {
                    //   await deleteWorkspace(
                    //     mergeWorkspace.repo_path,
                    //     mergeWorkspace.workspace_path,
                    //     mergeWorkspace.id
                    //   );
                    //   // Invalidate workspace queries
                    //   void invalidateQueries(["workspaces"]);
                    // } catch (error) {
                    //   addToast({
                    //     title: "Merge succeeded but workspace deletion failed",
                    //     description: "Please manually delete the workspace from the sidebar",
                    //     type: "warning",
                    //   });
                    // } finally {
                    setMergeWorkspace(null);
                    setViewMode("show-workspace");
                    handleReturnToDashboard();
                    void invalidateQueries(["repo-status", queryRepoKey]);
                    void invalidateQueries(["repo-branch", queryRepoKey]);
                    void invalidateQueries(["workspaces", queryRepoKey]);
                    void invalidateQueries([
                      "workspace-statuses",
                      queryRepoKey,
                    ]);
                    // }
                  }}
                />
              )}
            </div>
          </SidebarInset>

          {/* Global Dialogs */}
          {/* Note: MergeDialog removed - git-specific feature */}

          <AlertDialog
            open={pendingChangeMove !== null}
            onOpenChange={(open) => {
              if (!open && !changeMovePending) setPendingChangeMove(null);
            }}
          >
            <AlertDialogContent>
              <AlertDialogHeader>
                <AlertDialogTitle>
                  {pendingChangeMove
                    ? `Move ${pendingChangeMove.files.length} ${
                        pendingChangeMove.files.length === 1 ? "file" : "files"
                      } to ${pendingChangeMove.destinationLabel}?`
                    : "Move files?"}
                </AlertDialogTitle>
                <AlertDialogDescription>
                  This moves the selected uncommitted{" "}
                  {pendingChangeMove?.files.length === 1 ? "change" : "changes"}{" "}
                  into the target workspace and removes{" "}
                  {pendingChangeMove?.files.length === 1 ? "it" : "them"} from
                  the current one.
                </AlertDialogDescription>
              </AlertDialogHeader>
              <AlertDialogFooter>
                <AlertDialogCancel disabled={changeMovePending}>
                  Cancel
                </AlertDialogCancel>
                <AlertDialogAction
                  disabled={changeMovePending}
                  onClick={(e) => {
                    e.preventDefault();
                    void handleConfirmChangeMove();
                  }}
                >
                  Move
                </AlertDialogAction>
              </AlertDialogFooter>
            </AlertDialogContent>
          </AlertDialog>

          <UnifiedWorkspaceDialog
            open={unifiedDialogDefaults !== null}
            onOpenChange={(open) => {
              if (!open) setUnifiedDialogDefaults(null);
            }}
            repoPath={dataRepoPath}
            defaults={unifiedDialogDefaults ?? {}}
            onSuccess={async (workspaceId) => {
              await invalidateQueries(["workspaces", queryRepoKey]);
              invalidateQueries(["workspace-statuses", queryRepoKey]);
              const updatedWorkspaces = await fetchAndCache(
                ["workspaces", queryRepoKey],
                () => getWorkspaces(dataRepoPath),
              );
              const newWorkspace = updatedWorkspaces.find(
                (w) => w.id === workspaceId,
              );
              if (newWorkspace) {
                handleOpenSession(newWorkspace);
              }
            }}
          />

          <CommandPalette
            showCommandPalette={showCommandPalette}
            onCommandPaletteChange={setShowCommandPalette}
            workspaces={workspaces}
            onNavigateToDashboard={handleReturnToDashboard}
            onNavigateToSettings={openSettings}
            onOpenRepository={() => void handleOpenRepository()}
            onOpenRepositoryInNewWindow={() =>
              void handleOpenRepositoryInNewWindow()
            }
            onOpenBranchSwitcher={() => setShowBranchSwitcher(true)}
            onOpenFilePicker={() => setShowFilePicker(true)}
            onOpenWorkspacePicker={() => setShowWorkspacePicker(true)}
            onOpenWorkspaceDeletion={() => setShowWorkspaceDeletion(true)}
            onCreateStackedWorkspace={handleCreateStackedWorkspace}
            onToggleTerminal={() => terminalPaneRef.current?.toggleCollapse()}
            onMaximizeTerminal={() => terminalPaneRef.current?.toggleMaximize()}
            onStartAgentWithPrompt={() => setShowAgentPromptDialog(true)}
            onStartAgentTerminal={() => void handleStartDefaultAgent()}
            onOpenPromptHistory={() => {
              setPromptHistoryFocusId(null);
              setShowPromptHistory(true);
            }}
            onOpenStash={() => setShowStashModal(true)}
            onCreateShellTerminal={() =>
              terminalPaneRef.current?.createShellSession()
            }
            hasSelectedWorkspace={!!selectedWorkspace}
            showBranchSwitcher={showBranchSwitcher}
            onBranchSwitcherChange={setShowBranchSwitcher}
            onBranchChanged={handleBranchChanged}
            showWorkspaceDeletion={showWorkspaceDeletion}
            onWorkspaceDeletionChange={setShowWorkspaceDeletion}
            currentWorkspace={selectedWorkspace}
            onDeleteWorkspace={handleDelete}
            showFilePicker={showFilePicker}
            onFilePickerChange={setShowFilePicker}
            onFileSelected={(filePath) => setSessionSelectedFile(filePath)}
            selectedWorkspaceId={selectedWorkspace?.id ?? null}
            repoPath={dataRepoPath}
          />

          <AgentPromptDialog
            open={showAgentPromptDialog}
            onOpenChange={handleAgentPromptDialogOpenChange}
            repoPath={dataRepoPath}
            defaultBranch={effectiveDefaultBranch}
            workspaces={workspaces}
            onSessionCreated={handleSessionCreated}
            initialPrompt={runPromptRequest?.prompt}
            initialWorkspaceId={runPromptRequest?.workspaceId ?? null}
            initialIssue={runPromptRequest?.issue ?? null}
          />

          <PromptHistoryModal
            open={showPromptHistory}
            onOpenChange={handlePromptHistoryOpenChange}
            repoPath={dataRepoPath}
            initialSelectedId={promptHistoryFocusId}
            onRunPrompt={handleRunPrompt}
          />

          <KeyboardShortcutsModal
            open={showKeyboardShortcuts}
            onOpenChange={setShowKeyboardShortcuts}
          />

          <StashModal
            open={showStashModal}
            onOpenChange={setShowStashModal}
            repoPath={dataRepoPath}
            workspaces={visibleWorkspaces}
            onApplied={() => {
              void invalidateQueries(["workspace-changed-files"]);
              void invalidateQueries(["workspace-diff"]);
            }}
            onApplyToNewWorkspace={(entry) => {
              setShowStashModal(false);
              const source =
                entry.workspace_id != null
                  ? (workspaces.find((w) => w.id === entry.workspace_id) ??
                    null)
                  : null;
              setUnifiedDialogDefaults({
                sourceWorkspace: source,
                targetBranch: source?.branch_name,
                applyStashId: entry.id,
                applyStashCommit: {
                  hash: entry.short_commit_id,
                  message: `Stash from ${entry.workspace_label}`,
                  timestamp: entry.created_at,
                },
                preSelectedCommits: [entry.short_commit_id],
                activeTab: "commits",
                description: `Apply stash ${entry.short_commit_id} (${entry.files_changed.length} files, +${entry.additions}/-${entry.deletions})`,
              });
            }}
          />

          <WorkspacePicker
            open={showWorkspacePicker}
            onOpenChange={setShowWorkspacePicker}
            workspaces={workspaces}
            sessions={sessions}
            workspaceChangeCounts={undefined}
            onSelect={handleOpenSession}
          />
          {remoteSetupDialog}
          <RemoteAmbiguousMutationDialog />
          {activeSshEndpoint && activeRepository?.canonicalPath && (
            <RemoteTerminalDialog
              open={showRemoteTerminalDialog}
              onOpenChange={setShowRemoteTerminalDialog}
              endpoint={activeSshEndpoint}
              repositoryId={activeRepository.canonicalPath}
              {...resolveRemoteTerminalTarget(
                activeRepository.canonicalPath,
                isRemoteActive && selectedWorkspace
                  ? {
                      workspace_name: selectedWorkspace.workspace_name,
                      workspace_path: getFullWorkspacePath(selectedWorkspace),
                    }
                  : null,
              )}
              onOpenTarget={(target) =>
                terminalPaneRef.current?.openRemoteSession(target)
              }
            />
          )}
        </SidebarProvider>
      )}
    </ActiveRepositoryProvider>
  );
};
