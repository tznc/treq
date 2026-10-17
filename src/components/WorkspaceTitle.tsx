import { Pencil } from "lucide-react";
import { useState } from "react";
import { updateWorkspace } from "../lib/api";
import { invalidateQueries } from "../lib/swr-cache";
import { Button } from "./ui/button";
import { Textarea } from "./ui/textarea";
import { useToast } from "./ui/toast";

export function WorkspaceTitle({
  repoPath,
  workspaceId,
  title,
}: {
  repoPath: string;
  workspaceId: number;
  title: string;
}) {
  const { addToast } = useToast();
  const [draft, setDraft] = useState<string | null>(null);
  const [saving, setSaving] = useState(false);

  const save = async () => {
    if (draft == null || saving || !draft.trim()) return;
    setSaving(true);
    try {
      await updateWorkspace(repoPath, workspaceId, undefined, draft.trim());
      await invalidateQueries(["workspaces"]);
      setDraft(null);
    } catch (error) {
      addToast({
        title: "Failed to save title",
        description: error instanceof Error ? error.message : String(error),
        type: "error",
      });
    } finally {
      setSaving(false);
    }
  };

  return (
    <div data-testid="workspace-title">
      {draft == null ? (
        <div className="flex items-start gap-2">
          <h1 className="min-w-0 flex-1 text-2xl font-semibold leading-tight text-foreground whitespace-pre-wrap [overflow-wrap:anywhere]">
            {title}
          </h1>
          <Button
            variant="ghost"
            size="icon"
            className="h-5 w-5 shrink-0 text-muted-foreground"
            aria-label="Edit workspace title"
            onClick={() => setDraft(title)}
          >
            <Pencil className="w-4 h-4" />
          </Button>
        </div>
      ) : (
        <div className="space-y-2">
          <Textarea
            autoResize
            autoFocus
            rows={1}
            aria-label="Workspace title"
            value={draft}
            disabled={saving}
            onChange={(e) => setDraft(e.target.value)}
            onKeyDown={(e) => {
              if (e.key === "Escape") setDraft(null);
              if (e.key === "Enter" && (e.metaKey || e.ctrlKey)) {
                e.preventDefault();
                void save();
              }
            }}
            className="text-2xl font-semibold"
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
            <Button
              size="sm"
              disabled={saving || !draft.trim()}
              onClick={() => void save()}
            >
              Save
            </Button>
          </div>
        </div>
      )}
    </div>
  );
}
