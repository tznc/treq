import { useState } from "react";
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
  /** Why the status failed to load; the editor stays disabled without it. */
  loadError?: string;
  onScriptChange: (script: string) => void;
  onAlwaysRunChange: (alwaysRun: boolean) => void;
  /** Starts a run; resolves once the refreshed status reports it. */
  onRun: () => Promise<void>;
}

export function AppSetupScriptSettings({
  script,
  alwaysRun,
  status,
  loadError,
  onScriptChange,
  onAlwaysRunChange,
  onRun,
}: AppSetupScriptSettingsProps) {
  // Set from the click until the status reports the run, so a second click
  // in between cannot start another run.
  const [starting, setStarting] = useState(false);
  // Before the status loads the props are defaults; an edit would overwrite.
  const loading = !status;
  // Run executes the saved script, so an unsaved edit must be saved first.
  const unsaved = !!status && script !== status.script;
  const canRun =
    !!status &&
    !status.running &&
    !starting &&
    !unsaved &&
    status.script.trim() !== "";

  async function handleRun() {
    setStarting(true);
    try {
      await onRun();
    } finally {
      setStarting(false);
    }
  }
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
        <Button
          size="sm"
          variant="outline"
          disabled={!canRun}
          onClick={handleRun}
        >
          <Play className="h-3 w-3 mr-1" />
          Run
        </Button>
        {loadError ? (
          <p className="text-sm text-destructive">
            Could not load the setup script: {loadError}
          </p>
        ) : (
          <p className="text-sm text-muted-foreground">
            {unsaved
              ? "Save settings to run the edited script."
              : formatStatus(status)}
          </p>
        )}
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
