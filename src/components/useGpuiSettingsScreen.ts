import { useState } from "react";
import {
  type GpuiAccountState,
  type GpuiScreenAction,
  type GpuiSettingsTab,
  useGpuiScreen,
} from "../lib/gpui-screen";
import { useAuthStore } from "../stores/authStore";
import { useToastStore } from "../stores/toastStore";
import type { CloudWorkspaceControls } from "./AccountSettings";
import { useGitHubRepositories } from "./GitHubIntegrationSettings";
import { startLinearOAuth } from "./LinearIntegrationSettings";

type SettingsTabValue =
  | "application"
  | "repository"
  | "account"
  | "integrations"
  | "skills"
  | "preview";

/** The cloud workspace card's buttons GPUI hands back, by press id. */
const CLOUD_ACTIONS = {
  "cloud.provision": "onProvision",
  "cloud.wake": "onWake",
  "cloud.repair": "onRepair",
  "cloud.delete": "onDelete",
} as const;

function errorMessage(error: unknown, fallback: string): string {
  return error instanceof Error ? error.message : fallback;
}

/**
 * The auth store as the Account and Integrations tabs draw it, and the
 * handlers for what GPUI hands back from them.
 */
function useGpuiAccount(
  tab: SettingsTabValue,
  cloudWorkspace: CloudWorkspaceControls | undefined,
) {
  const {
    user,
    session,
    loading,
    signingIn,
    signInError,
    subscription,
    signOut,
    exchangeToken,
  } = useAuthStore();
  const addToast = useToastStore((s) => s.addToast);
  const [devLoading, setDevLoading] = useState(false);
  const [devError, setDevError] = useState<string | null>(null);
  const [cloudSubmitting, setCloudSubmitting] = useState(false);
  const [linearOauthLoading, setLinearOauthLoading] = useState(false);
  const signedIn = Boolean(user && session);
  const repositories = useGitHubRepositories(
    signedIn && tab === "integrations" ? user?.id : null,
  );

  const shown = tab === "account" || tab === "integrations";
  const metadata = user?.user_metadata;
  const state: GpuiAccountState | null = shown
    ? {
        loading,
        signingIn,
        signInError,
        user:
          user && session
            ? {
                name: metadata?.full_name || metadata?.name || "User",
                email: user.email ?? null,
                avatarUrl: metadata?.avatar_url || metadata?.picture || null,
              }
            : null,
        subscription: subscription
          ? {
              plan: subscription.plan,
              status: subscription.status,
              currentPeriodEnd: subscription.current_period_end,
            }
          : null,
        devCallback: { loading: devLoading, error: devError },
        cloud: cloudWorkspace
          ? {
              instance: cloudWorkspace.instanceStatus?.instance
                ? {
                    status: cloudWorkspace.instanceStatus.instance.status,
                    providerResourceId:
                      cloudWorkspace.instanceStatus.instance
                        .provider_resource_id,
                    diskQuotaGb:
                      cloudWorkspace.instanceStatus.instance.disk_quota_gb,
                  }
                : null,
              endpoint: cloudWorkspace.instanceStatus?.endpoint
                ? {
                    username: cloudWorkspace.instanceStatus.endpoint.username,
                    hostname: cloudWorkspace.instanceStatus.endpoint.hostname,
                    port: cloudWorkspace.instanceStatus.endpoint.port,
                  }
                : null,
              provisioningStage: cloudWorkspace.provisioningStage ?? null,
              provisioningError: cloudWorkspace.provisioningError ?? null,
              usage: cloudWorkspace.usage
                ? {
                    repositoryCount: cloudWorkspace.usage.repository_count,
                    workspaceCount: cloudWorkspace.usage.workspace_count,
                    diskUsedBytes: cloudWorkspace.usage.disk_used_bytes,
                  }
                : null,
              usageError: cloudWorkspace.usageError ?? null,
              submitting: cloudSubmitting,
            }
          : null,
        github:
          signedIn && tab === "integrations"
            ? {
                loading: repositories.isLoading,
                failed: Boolean(repositories.error),
                repositories: (repositories.data ?? []).map((repo) => ({
                  id: repo.id,
                  fullName: repo.full_name,
                  private: repo.private,
                })),
              }
            : null,
        linearOauthLoading,
      }
    : null;

  const runCloud = async (action: () => Promise<void>) => {
    setCloudSubmitting(true);
    try {
      await action();
    } finally {
      setCloudSubmitting(false);
    }
  };

  const exchange = async (token: string) => {
    setDevError(null);
    setDevLoading(true);
    try {
      await exchangeToken(token);
    } catch (error) {
      setDevError(errorMessage(error, "Token exchange failed"));
    } finally {
      setDevLoading(false);
    }
  };

  const connectLinear = async () => {
    setLinearOauthLoading(true);
    try {
      await startLinearOAuth(addToast);
    } finally {
      setLinearOauthLoading(false);
    }
  };

  /** Runs `action` if it is one of the account's; false otherwise. */
  const handle = (action: GpuiScreenAction): boolean => {
    if (action.kind === "field" && action.field === "account.devCallback") {
      void exchange(String(action.value));
      return true;
    }
    if (action.kind !== "press") return false;
    if (action.id === "account.signOut") {
      void signOut();
    } else if (action.id === "linear.oauth") {
      void connectLinear();
    } else if (action.id === "cloud.openRepositories") {
      cloudWorkspace?.onOpenRepositories?.();
    } else if (action.id in CLOUD_ACTIONS) {
      const name = CLOUD_ACTIONS[action.id as keyof typeof CLOUD_ACTIONS];
      if (cloudWorkspace) void runCloud(cloudWorkspace[name]);
    } else {
      return false;
    }
    return true;
  };

  return { state, handle };
}

/**
 * Draws the settings page in GPUI, which loads and saves the settings
 * through the backend itself and tells React what changed
 * (`useGpuiNativeEvents`). React keeps the tab and Close, and the Supabase
 * session the Account and Integrations tabs show.
 */
export function useGpuiSettingsScreen(params: {
  currentTab: SettingsTabValue;
  setCurrentTab: (tab: SettingsTabValue) => void;
  repoPath: string;
  currentBranch: string | null;
  onClose: () => void;
  cloudWorkspace?: CloudWorkspaceControls;
}) {
  const { currentTab } = params;
  const account = useGpuiAccount(currentTab, params.cloudWorkspace);
  const view = {
    kind: "settings",
    tab: currentTab satisfies GpuiSettingsTab,
    repoPath: params.repoPath || null,
    currentBranch: params.currentBranch,
    account: account.state,
  } as const;

  useGpuiScreen(view, (action: GpuiScreenAction) => {
    if (account.handle(action)) return;
    if (action.kind === "press" && action.id === "settings.close") {
      params.onClose();
    } else if (action.kind === "field" && action.field === "settings.tab") {
      params.setCurrentTab(action.value as SettingsTabValue);
    }
  });
}
