import { useImperativeHandle, useState, type Ref } from "react";
import useSWR from "swr";
import { useRepoYamlConfig } from "../hooks/useRepoYamlConfig";
import { getRepoSetting, setRepoSetting } from "../lib/api";
import { Input } from "./ui/input";
import { Label } from "./ui/label";
import { Switch } from "./ui/switch";
import { Textarea } from "./ui/textarea";
import { useToast } from "./ui/toast";
import { AgentOptions } from "./AgentOptions";

interface RepositorySettingsContentProps {
  repoPath: string;
  onSavingChange?: (saving: boolean) => void;
  ref?: Ref<RepositorySettingsContentHandle>;
}

export interface RepositorySettingsContentHandle {
  save: () => Promise<void>;
}

/**
 * Maps a stored `auto_review_trigger` onto the options this select offers.
 * `"on-push"` is the name the setting shipped with before anything fired it;
 * it meant "after remote changes arrive", which is now `"on-pull"`.
 */
const normalizeAutoReviewTrigger = (stored: string | null): string => {
  if (stored === "on-push") return "on-pull";
  if (
    stored === "on-commit" ||
    stored === "on-rebase" ||
    stored === "on-pull"
  ) {
    return stored;
  }
  return "off";
};

export const RepositorySettingsContent = ({
  repoPath,
  onSavingChange,
  ref,
}: RepositorySettingsContentProps) => {
  const { addToast } = useToast();
  const [saveError, setSaveError] = useState<string | null>(null);
  const [draft, setDraft] = useState<{
    key: string;
    branchNamePattern: string;
    includedFiles: string;
    defaultModel: string;
    defaultAgent: string;
    autoPush: boolean;
    ignoreGeneratedAgentFiles: boolean;
    reviewPrompt: string;
    reviewAgent: string;
    autoReviewTrigger: string;
  } | null>(null);

  const {
    data: loaded,
    error: loadError,
    isLoading: loading,
  } = useSWR(repoPath ? ["repo-settings", repoPath] : null, async () => {
    const [
      branchPattern,
      includedPatterns,
      model,
      agent,
      autoPushSetting,
      ignoreGeneratedSetting,
      reviewPromptSetting,
      reviewAgentSetting,
      autoReviewTriggerSetting,
    ] = await Promise.all([
      getRepoSetting(repoPath, "branch_name_pattern"),
      getRepoSetting(repoPath, "included_copy_files"),
      getRepoSetting(repoPath, "default_model"),
      getRepoSetting(repoPath, "default_agent"),
      getRepoSetting(repoPath, "auto_push"),
      getRepoSetting(repoPath, "ignore_generated_treq_paths"),
      getRepoSetting(repoPath, "review_prompt"),
      getRepoSetting(repoPath, "review_agent"),
      getRepoSetting(repoPath, "auto_review_trigger"),
    ]);
    return {
      branchNamePattern: branchPattern || "treq/{name}",
      includedFiles: includedPatterns || "",
      defaultModel: model || "",
      defaultAgent: agent || "",
      autoPush: autoPushSetting === "true",
      ignoreGeneratedAgentFiles: ignoreGeneratedSetting === "true",
      reviewPrompt: reviewPromptSetting || "",
      reviewAgent: reviewAgentSetting || "",
      autoReviewTrigger: normalizeAutoReviewTrigger(autoReviewTriggerSetting),
    };
  });

  const settings =
    draft?.key === repoPath
      ? draft
      : (loaded ?? {
          branchNamePattern: "treq/{name}",
          includedFiles: "",
          defaultModel: "",
          defaultAgent: "",
          autoPush: false,
          ignoreGeneratedAgentFiles: false,
          reviewPrompt: "",
          reviewAgent: "",
          autoReviewTrigger: "off",
        });
  const {
    branchNamePattern,
    includedFiles,
    defaultModel,
    defaultAgent,
    autoPush,
    ignoreGeneratedAgentFiles,
    reviewPrompt,
    reviewAgent,
    autoReviewTrigger,
  } = settings;

  // .treq/config.yaml, when it sets a field, is synced into these same repo
  // settings on the backend — so `settings` above already reflects it. This
  // is only used to know which fields to disable and explain via the card.
  const { config: yamlConfig, loading: yamlConfigLoading } =
    useRepoYamlConfig(repoPath);
  const branchNamePatternManaged = yamlConfig?.branch_name_pattern != null;
  const includedFilesManaged = yamlConfig?.included_copy_files != null;
  const defaultModelManaged = yamlConfig?.default_model != null;
  const defaultAgentManaged = yamlConfig?.default_agent != null;
  const reviewPromptManaged = yamlConfig?.review_prompt != null;
  const reviewAgentManaged = yamlConfig?.review_agent != null;
  const autoReviewTriggerManaged = yamlConfig?.auto_review_trigger != null;

  const error =
    saveError ?? (loadError ? `Failed to load settings: ${loadError}` : null);

  const updateDraft = (
    patch: Partial<{
      branchNamePattern: string;
      includedFiles: string;
      defaultModel: string;
      defaultAgent: string;
      autoPush: boolean;
      ignoreGeneratedAgentFiles: boolean;
      reviewPrompt: string;
      reviewAgent: string;
      autoReviewTrigger: string;
    }>,
  ) => {
    setDraft({
      key: repoPath,
      branchNamePattern,
      includedFiles,
      defaultModel,
      defaultAgent,
      autoPush,
      ignoreGeneratedAgentFiles,
      reviewPrompt,
      reviewAgent,
      autoReviewTrigger,
      ...patch,
    });
  };

  const setBranchNamePattern = (v: string) =>
    updateDraft({ branchNamePattern: v });
  const setIncludedFiles = (v: string) => updateDraft({ includedFiles: v });
  const setDefaultModel = (v: string) => updateDraft({ defaultModel: v });
  const setDefaultAgent = (v: string) => updateDraft({ defaultAgent: v });
  const setAutoPush = (v: boolean) => updateDraft({ autoPush: v });
  const setIgnoreGeneratedAgentFiles = (v: boolean) =>
    updateDraft({ ignoreGeneratedAgentFiles: v });
  const setReviewPrompt = (v: string) => updateDraft({ reviewPrompt: v });
  const setReviewAgent = (v: string) => updateDraft({ reviewAgent: v });
  const setAutoReviewTrigger = (v: string) =>
    updateDraft({ autoReviewTrigger: v });

  const handleSave = async () => {
    onSavingChange?.(true);

    try {
      await Promise.all([
        setRepoSetting(repoPath, "branch_name_pattern", branchNamePattern),
        setRepoSetting(repoPath, "included_copy_files", includedFiles),
        setRepoSetting(repoPath, "default_model", defaultModel),
        setRepoSetting(repoPath, "default_agent", defaultAgent),
        setRepoSetting(repoPath, "auto_push", autoPush ? "true" : "false"),
        setRepoSetting(
          repoPath,
          "ignore_generated_treq_paths",
          ignoreGeneratedAgentFiles ? "true" : "false",
        ),
        setRepoSetting(repoPath, "review_prompt", reviewPrompt),
        setRepoSetting(repoPath, "review_agent", reviewAgent),
        setRepoSetting(repoPath, "auto_review_trigger", autoReviewTrigger),
      ]);
      addToast({
        title: "Settings saved",
        description: "Repository settings have been updated successfully.",
        type: "success",
      });
    } catch (err) {
      const errorMsg = err instanceof Error ? err.message : String(err);
      setSaveError(`Failed to save settings: ${errorMsg}`);
      addToast({
        title: "Error",
        description: `Failed to save settings: ${errorMsg}`,
        type: "error",
      });
    } finally {
      onSavingChange?.(false);
    }
  };

  useImperativeHandle(ref, () => ({ save: handleSave }));

  if (loading || yamlConfigLoading) {
    return (
      <div className="py-8 text-center text-muted-foreground">
        Loading settings...
      </div>
    );
  }

  return (
    <div className="space-y-6">
      <div>
        <Label htmlFor="branch-name-pattern">Branch Name Pattern</Label>
        <Input
          id="branch-name-pattern"
          value={branchNamePattern}
          onChange={(e) => setBranchNamePattern(e.target.value)}
          placeholder="treq/{name}"
          className="mt-2 font-mono"
          disabled={branchNamePatternManaged}
        />
        <p className="text-sm text-muted-foreground mt-1">
          treq/{"{name}"} → treq/add-dark-mode
        </p>
      </div>

      <div>
        <Label htmlFor="included-files">Included Files/Directories</Label>
        <Textarea
          id="included-files"
          value={includedFiles}
          onChange={(e) => setIncludedFiles(e.target.value)}
          placeholder="e.g., .env&#10;.env.local"
          rows={4}
          className="font-mono text-sm mt-2"
          disabled={includedFilesManaged}
        />
        <p className="text-sm text-muted-foreground mt-1">
          Paths to copy into each new workspace (e.g. .env). For heavy dirs like
          node_modules, use Symlink from home repo under Advanced when creating
          a workspace.
        </p>
      </div>

      <div>
        <Label htmlFor="repo-default-model">Claude Code Model</Label>
        <select
          id="repo-default-model"
          value={defaultModel}
          onChange={(e) => setDefaultModel(e.target.value)}
          className="mt-2 w-full px-3 py-2 border rounded-md bg-background text-foreground disabled:cursor-not-allowed disabled:opacity-40"
          disabled={defaultModelManaged}
        >
          <option value="">Use Application Default</option>
          <option value="sonnet">Sonnet</option>
          <option value="opus">Opus</option>
          <option value="haiku">Haiku</option>
          <option value="sonnet[1m]">Sonnet (1M)</option>
          <option value="opusplan">Opus Plan</option>
        </select>
        <p className="text-sm text-muted-foreground mt-1">
          Default model for new Claude Code sessions in this repository
          (overrides application default)
        </p>
      </div>

      <div>
        <Label htmlFor="repo-default-agent">Default Agent</Label>
        <select
          id="repo-default-agent"
          value={defaultAgent}
          onChange={(e) => setDefaultAgent(e.target.value)}
          className="mt-2 w-full px-3 py-2 border rounded-md bg-background text-foreground disabled:cursor-not-allowed disabled:opacity-40"
          disabled={defaultAgentManaged}
        >
          <option value="">Use Application Default</option>
          <AgentOptions />
        </select>
        <p className="text-sm text-muted-foreground mt-1">
          Default agent for new sessions in this repository (overrides
          application default)
        </p>
      </div>

      <div className="flex items-center justify-between gap-4">
        <div>
          <Label htmlFor="auto-push">Auto-push to remote</Label>
          <p className="text-sm text-muted-foreground mt-1">
            Automatically push to the remote after every commit in this
            repository
          </p>
        </div>
        <Switch
          id="auto-push"
          checked={autoPush}
          onCheckedChange={setAutoPush}
        />
      </div>

      <div className="flex items-center justify-between gap-4">
        <div>
          <Label htmlFor="ignore-generated-agent-files">
            Ignore generated Treq paths
          </Label>
          <p className="text-sm text-muted-foreground mt-1">
            Add .jj*/ and Treq-generated agent skill paths to this
            repository&apos;s .gitignore
          </p>
        </div>
        <Switch
          id="ignore-generated-agent-files"
          checked={ignoreGeneratedAgentFiles}
          onCheckedChange={setIgnoreGeneratedAgentFiles}
        />
      </div>

      <div
        className="space-y-4 border-t border-border pt-6"
        data-testid="code-review-settings-section"
      >
        <div>
          <h3 className="text-sm font-medium">Code Review</h3>
          <p className="text-sm text-muted-foreground mt-1">
            Settings for the AI review agent. Its comments stay local to this
            machine and are never pushed to GitHub.
          </p>
        </div>

        <div>
          <Label htmlFor="review-prompt">Custom Review Prompt</Label>
          <Textarea
            id="review-prompt"
            value={reviewPrompt}
            onChange={(e) => setReviewPrompt(e.target.value)}
            placeholder="Leave empty to use the built-in review prompt"
            rows={6}
            className="font-mono text-sm mt-2"
            disabled={reviewPromptManaged}
          />
          <p className="text-sm text-muted-foreground mt-1">
            Overrides the built-in prompt. {"{target_type}"}, {"{target_id}"}{" "}
            and {"{diff_summary}"} are substituted before the prompt is sent.
          </p>
        </div>

        <div>
          <Label htmlFor="review-agent">Review Agent</Label>
          <select
            id="review-agent"
            value={reviewAgent}
            onChange={(e) => setReviewAgent(e.target.value)}
            className="mt-2 w-full px-3 py-2 border rounded-md bg-background text-foreground disabled:cursor-not-allowed disabled:opacity-40"
            disabled={reviewAgentManaged}
          >
            <option value="">Use Default Agent</option>
            <AgentOptions />
          </select>
          <p className="text-sm text-muted-foreground mt-1">
            Agent launched by Start Review (overrides the default agent)
          </p>
        </div>

        <div>
          <Label htmlFor="auto-review-trigger">Automatic Review</Label>
          <select
            id="auto-review-trigger"
            value={autoReviewTrigger}
            onChange={(e) => setAutoReviewTrigger(e.target.value)}
            className="mt-2 w-full px-3 py-2 border rounded-md bg-background text-foreground disabled:cursor-not-allowed disabled:opacity-40"
            disabled={autoReviewTriggerManaged}
          >
            <option value="off">Off</option>
            <option value="on-commit">On commit</option>
            <option value="on-rebase">On rebase</option>
            <option value="on-pull">On pull from remote</option>
          </select>
          <p className="text-sm text-muted-foreground mt-1">
            Opens a review terminal on its own after the chosen operation
            finishes in a workspace of this repository.
          </p>
        </div>
      </div>

      {error && <div className="text-sm text-destructive">{error}</div>}
    </div>
  );
};

RepositorySettingsContent.displayName = "RepositorySettingsContent";
