import { Pencil } from "lucide-react";
import { useState } from "react";
import { updateWorkspace } from "../lib/api";
import { invalidateQueries } from "../lib/swr-cache";
import { MarkdownContent } from "./MarkdownContent";
import { Button } from "./ui/button";
import { Textarea } from "./ui/textarea";
import { useToast } from "./ui/toast";

// Block syntax at a line start (heading, list, quote, fence, table row) or
// inline emphasis, code or a link anywhere.
const MARKDOWN_PATTERNS = [
  /^#{1,6}\s/m,
  /^\s*[-*+]\s/m,
  /^\s*\d+\.\s/m,
  /^>\s/m,
  /^```/m,
  /^\|.*\|\s*$/m,
  /\*\*[^*\n]+\*\*/,
  /`[^`\n]+`/,
  /\[[^\]\n]+\]\([^)\s]+\)/,
];

export function looksLikeMarkdown(text: string): boolean {
  return MARKDOWN_PATTERNS.some((pattern) => pattern.test(text));
}

interface WorkspaceDescriptionProps {
  repoPath: string;
  workspaceId: number;
  description: string;
}

/** The workspace description, which Treq also uses as the PR body. */
export function WorkspaceDescription({
  repoPath,
  workspaceId,
  description,
}: WorkspaceDescriptionProps) {
  const { addToast } = useToast();
  const [draft, setDraft] = useState<string | null>(null);
  const [saving, setSaving] = useState(false);

  const save = async () => {
    if (draft == null) return;
    setSaving(true);
    try {
      await updateWorkspace(repoPath, workspaceId, undefined, undefined, draft);
      await invalidateQueries(["workspaces"]);
      setDraft(null);
    } catch (error) {
      addToast({
        title: "Failed to save description",
        description: error instanceof Error ? error.message : String(error),
        type: "error",
      });
    } finally {
      setSaving(false);
    }
  };

  return (
    <div
      data-testid="workspace-description"
      className="relative rounded-lg border border-border px-4 py-3"
    >
      {draft == null ? (
        <>
          <Button
            variant="ghost"
            size="icon"
            className="absolute top-2 right-2 h-7 w-7 text-muted-foreground"
            aria-label="Edit workspace description"
            onClick={() => setDraft(description)}
          >
            <Pencil className="w-4 h-4" />
          </Button>
          {looksLikeMarkdown(description) ? (
            <MarkdownContent content={description} className="prose-sm pr-8" />
          ) : (
            <p className="text-sm text-foreground whitespace-pre-wrap pr-8">
              {description}
            </p>
          )}
        </>
      ) : (
        <div className="space-y-2">
          <Textarea
            autoResize
            autoFocus
            aria-label="Workspace description"
            value={draft}
            onChange={(e) => setDraft(e.target.value)}
            onKeyDown={(e) => {
              if (e.key === "Escape") setDraft(null);
              if (e.key === "Enter" && (e.metaKey || e.ctrlKey)) {
                e.preventDefault();
                void save();
              }
            }}
            className="text-sm font-sans"
          />
          <div className="flex justify-end gap-2">
            <Button
              variant="ghost"
              size="sm"
              disabled={saving}
              onClick={() => setDraft(null)}
            >
              Cancel
            </Button>
            <Button size="sm" disabled={saving} onClick={() => void save()}>
              Save
            </Button>
          </div>
        </div>
      )}
    </div>
  );
}
