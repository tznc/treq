import {
  AlertCircle,
  ChevronRight,
  CircleDot,
  ExternalLink,
  GitMerge,
  GitPullRequestDraft,
  Loader2,
} from "lucide-react";
import { openUrl } from "@tauri-apps/plugin-opener";
import type {
  GhIssue,
  GhLabel,
  GhPullRequest,
  PrCheckEntry,
} from "../../lib/api-types";
import {
  ciStateForBucket,
  ciStatusStyle,
  formatCheckDuration,
} from "../../lib/ci-status";
import { Button } from "../ui/button";
import { useToast } from "../ui/toast";

export function formatDate(iso: string) {
  try {
    return new Date(iso).toLocaleDateString(undefined, {
      year: "numeric",
      month: "short",
      day: "numeric",
    });
  } catch {
    return iso;
  }
}

export function OpenInWebButton({ url }: { url: string }) {
  return (
    <Button
      size="sm"
      variant="outline"
      className="text-base shrink-0 gap-1.5"
      onClick={() => openUrl(url)}
    >
      <ExternalLink className="w-4 h-4" />
      Open in Web
    </Button>
  );
}

export function StateChip({
  state,
  isDraft = false,
}: {
  state: string;
  isDraft?: boolean;
}) {
  const s = state.toUpperCase();
  if (isDraft && s === "OPEN") {
    return (
      <span className="inline-flex items-center gap-1 px-2 py-0.5 rounded-full text-base bg-muted text-muted-foreground">
        <GitPullRequestDraft className="w-3 h-3" />
        Draft
      </span>
    );
  }
  if (s === "OPEN") {
    return (
      <span className="inline-flex items-center gap-1 px-2 py-0.5 rounded-full text-base bg-green-500/20 text-green-600 dark:text-green-400">
        <CircleDot className="w-3 h-3" />
        Open
      </span>
    );
  }
  if (s === "MERGED") {
    return (
      <span className="inline-flex items-center gap-1 px-2 py-0.5 rounded-full text-base bg-purple-500/20 text-purple-600 dark:text-purple-400">
        <GitMerge className="w-3 h-3" />
        Merged
      </span>
    );
  }
  return (
    <span className="inline-flex items-center gap-1 px-2 py-0.5 rounded-full text-base bg-red-500/20 text-red-600 dark:text-red-400">
      <AlertCircle className="w-3 h-3" />
      Closed
    </span>
  );
}

export function LabelChip({ name, color }: GhLabel) {
  const hex = color.startsWith("#") ? color : `#${color}`;
  return (
    <span
      className="inline-block px-1.5 py-0.5 rounded text-base font-medium"
      style={{
        backgroundColor: `${hex}33`,
        color: hex,
        border: `1px solid ${hex}66`,
      }}
    >
      {name}
    </span>
  );
}

/** One row of the PR page's "Checks" list, styled identically to the workspace header's CiStatusIndicator. */
export function CheckEntryRow({ check }: { check: PrCheckEntry }) {
  const state = ciStateForBucket(check.bucket);
  const { Icon, textClassName } = ciStatusStyle(state);
  const duration =
    check.duration_secs != null
      ? formatCheckDuration(check.duration_secs)
      : null;

  const content = (
    <div className="flex items-center gap-2 py-1.5 px-2 text-base">
      <Icon
        className={`w-4 h-4 shrink-0 ${textClassName} ${state === "pending" ? "animate-spin" : ""}`}
      />
      <span className="flex-1 min-w-0 truncate">{check.name}</span>
      {duration && (
        <span className="shrink-0 text-sm text-muted-foreground tabular-nums">
          {duration}
        </span>
      )}
    </div>
  );

  if (!check.link) return content;
  return (
    <button
      type="button"
      className="w-full text-left hover:bg-muted/50 transition-colors rounded"
      onClick={() => openUrl(check.link)}
    >
      {content}
    </button>
  );
}

export function IssueListItem({
  issue,
  selected,
  onClick,
}: {
  issue: GhIssue;
  selected: boolean;
  onClick: () => void;
}) {
  return (
    <button
      type="button"
      onClick={onClick}
      className={`w-full text-left px-3 py-2.5 border-b border-border hover:bg-muted/50 transition-colors ${
        selected ? "bg-primary/10" : ""
      }`}
    >
      <div className="flex items-start gap-2">
        <div className="flex-1 min-w-0">
          <div className="flex items-center gap-2 min-w-0">
            <p className="text-lg font-medium truncate min-w-0">
              {issue.title}
            </p>
            <span className="text-base text-muted-foreground shrink-0">
              #{issue.number}
            </span>
          </div>
          <div className="flex items-center gap-2 mt-1 min-w-0 text-base text-muted-foreground">
            <StateChip state={issue.state} />
            <span className="shrink-0">{formatDate(issue.created_at)}</span>
          </div>
        </div>
        <ChevronRight className="w-4 h-4 text-muted-foreground shrink-0 mt-1" />
      </div>
    </button>
  );
}

export function PrListItem({
  pr,
  selected,
  onClick,
  hideBranches,
}: {
  pr: GhPullRequest;
  selected: boolean;
  onClick: () => void;
  hideBranches?: boolean;
}) {
  return (
    <button
      type="button"
      onClick={onClick}
      className={`w-full text-left px-3 py-2.5 border-b border-border hover:bg-muted/50 transition-colors ${
        selected ? "bg-primary/10" : ""
      }`}
    >
      <div className="flex items-start gap-2">
        <div className="flex-1 min-w-0">
          <div className="flex items-center gap-2 min-w-0">
            <p className="text-lg font-medium truncate min-w-0">{pr.title}</p>
            <span className="text-base text-muted-foreground shrink-0">
              #{pr.number}
            </span>
          </div>
          <div className="flex items-center gap-2 mt-1 min-w-0 text-base text-muted-foreground">
            <StateChip state={pr.state} isDraft={Boolean(pr.is_draft)} />
            <span className="truncate max-w-[10rem]" title="Author">
              {pr.author.login}
            </span>
            {!hideBranches && (
              <span className="font-mono inline-flex items-center gap-1 min-w-0">
                <span
                  className="truncate max-w-[16rem]"
                  title={pr.head_ref_name}
                >
                  {pr.head_ref_name}
                </span>
                <span className="shrink-0">→</span>
                <span
                  className="truncate max-w-[16rem]"
                  title={pr.base_ref_name}
                >
                  {pr.base_ref_name}
                </span>
              </span>
            )}
            <span className="shrink-0">{formatDate(pr.created_at)}</span>
          </div>
        </div>
        <ChevronRight className="w-4 h-4 text-muted-foreground shrink-0 mt-1" />
      </div>
    </button>
  );
}

/** Segmented Draft/Open/Closed/All control for the issue and PR lists. */
export function StateFilterButtons<T extends string>({
  options,
  value,
  onChange,
}: {
  options: { label: string; value: T }[];
  value: T;
  onChange: (value: T) => void;
}) {
  return (
    <div className="flex items-center gap-1 border border-border rounded-md p-0.5 bg-muted/30">
      {options.map((btn) => (
        <button
          key={btn.value}
          type="button"
          onClick={() => onChange(btn.value)}
          aria-pressed={value === btn.value}
          className={`text-base px-2 py-0.5 rounded transition-colors ${
            value === btn.value
              ? "bg-background shadow-sm text-foreground"
              : "text-muted-foreground hover:text-foreground"
          }`}
        >
          {btn.label}
        </button>
      ))}
    </div>
  );
}

export function LoadMoreButton({
  loading,
  onClick,
}: {
  loading: boolean;
  onClick: () => void;
}) {
  return (
    <div className="p-3">
      <Button
        variant="outline"
        className="w-full text-base"
        disabled={loading}
        onClick={onClick}
      >
        {loading ? <Loader2 className="w-4 h-4 mr-2 animate-spin" /> : null}
        Load more
      </Button>
    </div>
  );
}

export function EmptyState({
  icon: Icon,
  message,
}: {
  icon: React.ComponentType<{ className?: string }>;
  message: string;
}) {
  return (
    <div className="flex flex-col items-center justify-center py-12 text-center px-6 gap-2">
      <Icon className="w-6 h-6 text-muted-foreground" />
      <p className="text-base text-muted-foreground">{message}</p>
    </div>
  );
}

/** Load failure with the gh message, so auth or network problems are visible. */
export function ErrorState({
  title,
  error,
  onRetry,
}: {
  title: string;
  error: unknown;
  onRetry?: () => void;
}) {
  return (
    <div
      role="alert"
      className="flex flex-col items-center justify-center py-12 text-center px-6 gap-2"
    >
      <AlertCircle className="w-6 h-6 text-destructive" />
      <p className="text-base font-medium">{title}</p>
      <p className="text-base text-muted-foreground break-words max-w-full">
        {error instanceof Error ? error.message : String(error)}
      </p>
      {onRetry && (
        <Button
          variant="outline"
          size="sm"
          className="text-base mt-1"
          onClick={onRetry}
        >
          Try again
        </Button>
      )}
    </div>
  );
}

/** Returns an `onError` factory that reports a failed gh action as an error toast. */
/** gh failures reach the UI as an Error or as the Tauri command's string. */
export function ghErrorText(error: unknown): string {
  return error instanceof Error ? error.message : String(error);
}

export function useGhErrorToast() {
  const { addToast } = useToast();
  return (title: string) => (error: unknown) =>
    addToast({
      title,
      description: ghErrorText(error),
      type: "error",
    });
}
