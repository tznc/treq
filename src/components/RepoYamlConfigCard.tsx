import { useRepoYamlConfig } from "../hooks/useRepoYamlConfig";
import type { RepoYamlConfig } from "../lib/api-extra";
import { Button } from "./ui/button";
import { Card, CardContent } from "./ui/card";

interface RepoYamlConfigCardProps {
  repoPath: string;
}

function hasAnyValue(config: RepoYamlConfig): boolean {
  return (
    config.branch_name_pattern != null ||
    config.default_model != null ||
    config.default_agent != null ||
    config.target_branch != null ||
    (config.included_copy_files != null &&
      config.included_copy_files.length > 0)
  );
}

/**
 * Shown at the top of Settings > Repository whenever `.treq/config.yaml`
 * sets at least one field. The settings it covers are disabled inline
 * rather than listed here — this card only explains why and points at the
 * file to edit.
 */
export function RepoYamlConfigCard({ repoPath }: RepoYamlConfigCardProps) {
  const { config, loading, error, reload } = useRepoYamlConfig(repoPath);

  if (error) {
    return (
      <Card className="border-destructive/50">
        <CardContent className="flex items-center justify-between gap-4 p-4">
          <p className="text-sm text-destructive">{error}</p>
          <Button
            variant="outline"
            size="sm"
            onClick={() => void reload()}
            disabled={loading}
          >
            {loading ? "Reloading..." : "Reload"}
          </Button>
        </CardContent>
      </Card>
    );
  }

  if (!config || !hasAnyValue(config)) {
    return null;
  }

  return (
    <Card className="bg-muted/50" data-testid="repo-yaml-config-card">
      <CardContent className="flex items-center justify-between gap-4 p-4">
        <p className="text-sm">
          <span className="font-medium">Synced from .treq/config.yaml.</span>{" "}
          <span className="text-muted-foreground">
            Settings shown as disabled below are managed by that file — edit it
            directly to change them.
          </span>
        </p>
        <Button
          variant="outline"
          size="sm"
          onClick={() => void reload()}
          disabled={loading}
        >
          {loading ? "Reloading..." : "Reload"}
        </Button>
      </CardContent>
    </Card>
  );
}
