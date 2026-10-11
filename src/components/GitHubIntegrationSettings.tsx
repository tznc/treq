import { openUrl } from "@tauri-apps/plugin-opener";
import { ExternalLink, Github, Loader2 } from "lucide-react";
import { useState } from "react";
import useSWR from "swr";
import { useAuthStore } from "../stores/authStore";
import { isProSubscription } from "../lib/subscription";
import {
  useGitRemoteInfo,
  useMergeQueueEnabled,
  useSetMergeQueueEnabled,
} from "../hooks/useMergeQueueStatus";
import { FEATURES } from "../lib/features";
import { isProRequiredError } from "../lib/pro-required";
import { supabase, WEB_URL } from "../lib/supabase";
import { Button } from "./ui/button";
import { Switch } from "./ui/switch";

export interface GitHubRepository {
  id: number;
  full_name: string;
  private: boolean;
  default_branch: string;
  installation_id: number;
}

interface GitHubIntegrationSettingsProps {
  /** Repo whose integration settings this page controls. */
  repoPath?: string;
}

/** One row of an integration's settings: label, explanation, and control. */
const SettingRow: React.FC<{
  title: string;
  description: string;
  children?: React.ReactNode;
}> = ({ title, description, children }) => (
  <div className="flex items-start justify-between gap-4 py-3">
    <div className="min-w-0">
      <p className="font-medium">{title}</p>
      <p className="text-base text-muted-foreground">{description}</p>
    </div>
    <div className="flex items-center gap-2 shrink-0">{children}</div>
  </div>
);

/**
 * Per-repo merge queue opt-in, stored in Postgres. A repo with no config row
 * has never opted in, so this reads as off. Turning it on requires a Pro plan
 * and a repo the GitHub App is installed on, so an ineligible repo gets the
 * reason rather than a control that could only fail.
 */
const MergeQueueSetting: React.FC<{
  repoPath?: string;
  isPro: boolean;
  repositories: GitHubRepository[];
  repositoriesLoading: boolean;
}> = ({ repoPath, isPro, repositories, repositoriesLoading }) => {
  const { data: remoteInfo } = useGitRemoteInfo(repoPath);
  const { data: enabled, isLoading } = useMergeQueueEnabled(repoPath);
  const setEnabled = useSetMergeQueueEnabled(repoPath);
  const [error, setError] = useState<string | null>(null);
  // The server refused to turn the queue on because there is no Pro, even
  // if the subscription the app last read said otherwise.
  const [proRequired, setProRequired] = useState(false);
  const hasPro = isPro && !proRequired;

  const isLinked =
    !!remoteInfo &&
    repositories.some((repo) => repo.full_name === remoteInfo.full_name);
  const isEligible = hasPro && isLinked;

  async function apply(next: boolean) {
    setError(null);
    try {
      await setEnabled.mutateAsync(next);
    } catch (err) {
      if (isProRequiredError(err)) {
        setProRequired(true);
      } else {
        setError((err as Error).message);
      }
    }
  }

  function ineligibleReason(): string {
    if (!remoteInfo) return "This repository has no GitHub remote.";
    if (!hasPro) return "Upgrade to Pro to use the merge queue.";
    return `Install the Treq GitHub App on ${remoteInfo.full_name} to use the merge queue.`;
  }

  return (
    <div data-testid="merge-queue-setting">
      <SettingRow
        title="Merge queue"
        description={
          enabled
            ? "Queued branches merge automatically once CI passes."
            : isEligible || repositoriesLoading
              ? "Merge branches automatically once CI passes."
              : ineligibleReason()
        }
      >
        {repositoriesLoading || isLoading ? (
          <Loader2
            aria-label="Loading merge queue setting"
            className="w-4 h-4 animate-spin text-muted-foreground"
          />
        ) : enabled ? (
          <>
            {setEnabled.isPending && (
              <Loader2 className="w-3.5 h-3.5 animate-spin text-muted-foreground" />
            )}
            <Switch
              aria-label="Enable merge queue"
              checked
              disabled={setEnabled.isPending}
              onCheckedChange={(next) => void apply(next)}
            />
          </>
        ) : isEligible ? (
          <Button
            size="sm"
            className="text-base"
            disabled={setEnabled.isPending}
            onClick={() => void apply(true)}
          >
            {setEnabled.isPending ? (
              <Loader2 className="w-3.5 h-3.5 mr-1 animate-spin" />
            ) : null}
            Enable merge queue
          </Button>
        ) : !hasPro ? (
          <Button
            size="sm"
            variant="outline"
            className="text-base"
            onClick={() => openUrl(`${WEB_URL}/dashboard`)}
          >
            Upgrade to Pro
          </Button>
        ) : null}
      </SettingRow>
      {error && <p className="text-base text-destructive pb-3">{error}</p>}
    </div>
  );
};

export const GitHubIntegrationSettings: React.FC<
  GitHubIntegrationSettingsProps
> = ({ repoPath }) => {
  const {
    user,
    session,
    loading: authLoading,
    subscription,
    signIn,
  } = useAuthStore();
  const userId = user?.id;
  const isSignedIn = !!user && !!session;
  const isPro = isProSubscription(subscription);
  const {
    data: repositories = [],
    isLoading: loading,
    error: reposError,
  } = useSWR(isSignedIn ? ["github-repositories", userId] : null, async () => {
    const { data, error } = await supabase
      .from("github_repositories")
      .select("id, full_name, private, default_branch, installation_id");
    if (error) throw error;
    return (data ?? []) as GitHubRepository[];
  });
  const failed = Boolean(reposError);

  if (authLoading) {
    return (
      <div className="flex justify-center py-16">
        <Loader2
          aria-label="Loading account"
          className="w-6 h-6 animate-spin text-muted-foreground"
        />
      </div>
    );
  }

  if (!isSignedIn) {
    return (
      <div className="flex flex-col items-center justify-center py-16 gap-4 text-center">
        <Github className="w-12 h-12 text-muted-foreground" />
        <div>
          <h3 className="text-lg font-semibold">
            Sign in to manage integrations
          </h3>
          <p className="text-base text-muted-foreground mt-1">
            Connect your GitHub repositories through Treq.
          </p>
        </div>
        <Button onClick={signIn} className="gap-2">
          <ExternalLink className="w-4 h-4" />
          Sign in with Browser
        </Button>
      </div>
    );
  }

  const visibleRepositories = isPro
    ? repositories
    : repositories.filter((repo) => !repo.private);

  return (
    <div className="space-y-10">
      {/* One header per integration; settings sit directly beneath it. */}
      <section>
        <div className="flex items-center gap-3 pb-2 border-b border-border">
          <Github className="w-5 h-5" />
          <h3 className="font-semibold">GitHub</h3>
          <span className="text-base text-muted-foreground">
            {isPro ? "All repositories" : "Public repositories"}
          </span>
        </div>

        <div className="divide-y divide-border">
          {FEATURES.mergeQueue && (
            <MergeQueueSetting
              repoPath={repoPath}
              isPro={isPro}
              repositories={repositories}
              repositoriesLoading={loading}
            />
          )}

          <SettingRow
            title="Connected repositories"
            description={
              loading
                ? "Loading GitHub repositories…"
                : failed
                  ? "Could not load GitHub repositories. Try again later."
                  : visibleRepositories.length === 0
                    ? "No enabled GitHub repositories."
                    : `${visibleRepositories.length} enabled.`
            }
          >
            <Button
              variant="outline"
              size="sm"
              className="gap-2 text-base"
              onClick={() => openUrl(`${WEB_URL}/dashboard?tab=integrations`)}
            >
              <ExternalLink className="w-3 h-3" />
              Manage GitHub
            </Button>
          </SettingRow>

          {!loading && !failed && visibleRepositories.length > 0 && (
            <ul className="py-2">
              {visibleRepositories.map((repo) => (
                <li
                  key={repo.id}
                  className="flex items-center justify-between py-1.5 text-base"
                >
                  <span className="font-mono">{repo.full_name}</span>
                  <span className="text-muted-foreground">
                    {repo.private ? "Private" : "Public"}
                  </span>
                </li>
              ))}
            </ul>
          )}
        </div>
      </section>
    </div>
  );
};
