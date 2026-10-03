import { Play } from "lucide-react";
import type { AppSetupScriptStatus } from "../lib/api";
import { Button } from "./ui/button";
import { Label } from "./ui/label";
import { Switch } from "./ui/switch";
import { Textarea } from "./ui/textarea";

interface AppSetupScriptSettingsProps {
  script: string;
  alwaysRun: boolean;
  status?: AppSetupScriptStatus;
  onScriptChange: (script: string) => void;
  onAlwaysRunChange: (alwaysRun: boolean) => void;
  onRun: () => void;
}

export function AppSetupScriptSettings({
  script,
  alwaysRun,
  status,
  onScriptChange,
  onAlwaysRunChange,
  onRun,
}: AppSetupScriptSettingsProps) {
  // Before the status loads the props are defaults; an edit would overwrite.
  const loading = !status;
  // Run executes the saved script, so an unsaved edit must be saved first.
  const unsaved = !!status && script !== status.script;
  const canRun =
    !!status && !status.running && !unsaved && status.script.trim() !== "";
  return (
    <div>
      <Label htmlFor="app-setup-script">Application Setup Script</Label>
      <Textarea
        id="app-setup-script"
        value={script}
        disabled={loading}
        onChange={(e) => onScriptChange(e.target.value)}
        placeholder="brew install jj"
        className="mt-2 font-mono"
        rows={4}
      />
      <p className="text-sm text-muted-foreground mt-1">
        Shell script run from your home directory when you click Run. Its output
        is in the Logs tab under App setup.
      </p>
      <div className="flex items-center gap-2 mt-3">
        <Switch
          id="app-setup-script-always-run"
          aria-label="Run on every app startup"
          checked={alwaysRun}
          disabled={loading}
          onCheckedChange={onAlwaysRunChange}
        />
        <Label htmlFor="app-setup-script-always-run">
          Run on every app startup
        </Label>
      </div>
      <div className="flex items-center gap-3 mt-3">
        <Button size="sm" variant="outline" disabled={!canRun} onClick={onRun}>
          <Play className="h-3 w-3 mr-1" />
          Run
        </Button>
        <p className="text-sm text-muted-foreground">
          {unsaved
            ? "Save settings to run the edited script."
            : formatStatus(status)}
        </p>
      </div>
    </div>
  );
}

function formatStatus(status?: AppSetupScriptStatus): string {
  if (!status) return "";
  if (status.running) return "Running…";
  if (!status.last_run_at) return "Never run";
  const ranAt = new Date(status.last_run_at).toLocaleString();
  return `Last run ${ranAt} · ${status.last_status ?? "unknown"}`;
}
