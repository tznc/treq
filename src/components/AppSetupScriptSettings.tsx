import type { AppSetupScriptStatus } from "../lib/api";
import { Label } from "./ui/label";
import { Switch } from "./ui/switch";
import { Textarea } from "./ui/textarea";

interface AppSetupScriptSettingsProps {
  script: string;
  alwaysRun: boolean;
  status?: AppSetupScriptStatus;
  onScriptChange: (script: string) => void;
  onAlwaysRunChange: (alwaysRun: boolean) => void;
}

export function AppSetupScriptSettings({
  script,
  alwaysRun,
  status,
  onScriptChange,
  onAlwaysRunChange,
}: AppSetupScriptSettingsProps) {
  return (
    <div>
      <Label htmlFor="app-setup-script">Application Setup Script</Label>
      <Textarea
        id="app-setup-script"
        value={script}
        onChange={(e) => onScriptChange(e.target.value)}
        placeholder="brew install jj"
        className="mt-2 font-mono"
        rows={4}
      />
      <p className="text-sm text-muted-foreground mt-1">
        Shell script run from your home directory. It runs once, and again
        whenever you change it.
      </p>
      <div className="flex items-center gap-2 mt-3">
        <Switch
          id="app-setup-script-always-run"
          aria-label="Run on every app startup"
          checked={alwaysRun}
          onCheckedChange={onAlwaysRunChange}
        />
        <Label htmlFor="app-setup-script-always-run">
          Run on every app startup
        </Label>
      </div>
      <p className="text-sm text-muted-foreground mt-2">
        {formatStatus(status)}
      </p>
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
