import { CircleDot, X } from "lucide-react";
import {
  ISSUE_SOURCES,
  type IssueAttachment,
  type IssueSource,
} from "../../lib/promptAttachments";

const ICON_CLASS: Record<IssueSource, string> = {
  github: "text-green-600 dark:text-green-400",
  linear: "text-violet-600 dark:text-violet-400",
};

/** The chip for an issue attached to a prompt, the same for every tracker. */
export const IssueChip: React.FC<{
  issue: IssueAttachment;
  onRemove: () => void;
}> = ({ issue, onRemove }) => {
  const { label, itemNoun } = ISSUE_SOURCES[issue.source];
  return (
    <div className="flex flex-wrap items-center gap-1.5 px-3 pt-3 pb-0">
      <span
        data-testid="issue-chip"
        data-source={issue.source}
        className="inline-flex items-center gap-1 rounded-md border border-border bg-muted/50 px-2 py-0.5 text-xs text-foreground"
        title={issue.title}
      >
        <CircleDot className={`h-3 w-3 ${ICON_CLASS[issue.source]}`} />
        <span className="font-medium">
          {label} {issue.key}
        </span>
        <button
          type="button"
          aria-label={`Remove ${label} ${itemNoun}`}
          className="ml-0.5 rounded-sm p-0.5 text-muted-foreground hover:bg-muted hover:text-foreground"
          onClick={onRemove}
        >
          <X className="h-3 w-3" />
        </button>
      </span>
    </div>
  );
};
