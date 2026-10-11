import { openUrl } from "@tauri-apps/plugin-opener";
import { Cloud, Loader2, Trash2, Wrench } from "lucide-react";
import { useState } from "react";
import type {
  InstanceStatusResponse,
  MachineUsageReport,
  ManagedInstanceState,
} from "../../lib/api-types-remote";
import { isProRequiredError, proRequiredMessage } from "../../lib/pro-required";
import { WEB_URL } from "../../lib/supabase";
import {
  AlertDialog,
  AlertDialogAction,
  AlertDialogCancel,
  AlertDialogContent,
  AlertDialogDescription,
  AlertDialogFooter,
  AlertDialogHeader,
  AlertDialogTitle,
} from "../ui/alert-dialog";
import { Button } from "../ui/button";
import {
  Tooltip,
  TooltipContent,
  TooltipProvider,
  TooltipTrigger,
} from "../ui/tooltip";
import { STAGE_LABELS } from "./remoteSetupLabels";

export interface CloudWorkspaceCardProps {
  instanceStatus: InstanceStatusResponse | null;
  provisioningStage?: string;
  provisioningError?: string;
  onProvision: () => Promise<void>;
  onWake: () => Promise<void>;
  onRepair: () => Promise<void>;
  onDelete: () => Promise<void>;
  onOpenRepositories?: () => void;
  /** Usage reported by the cloud workspace; unset while loading. */
  usage?: MachineUsageReport;
  /** Why usage could not be loaded, if it failed. */
  usageError?: string;
}

/** States where Treq detected a problem that Repair can fix in place. */
const REPAIRABLE_STATES: ReadonlySet<ManagedInstanceState> = new Set([
  "degraded",
  "failed",
]);

/**
 * The account's single Treq-managed cloud workspace: create it, see its
 * state, and repair or delete it. Rendered inline on the account settings
 * page and inside the remote setup dialog.
 */
const GIB = 1024 ** 3;
const MIB = 1024 ** 2;

function formatBytes(bytes: number): string {
  return bytes >= GIB
    ? `${(bytes / GIB).toFixed(1)} GB`
    : `${Math.round(bytes / MIB)} MB`;
}

function plural(count: number, singular: string, pluralForm: string): string {
  return `${count} ${count === 1 ? singular : pluralForm}`;
}

function CloudWorkspaceUsage({
  usage,
  usageError,
  diskQuotaGb,
}: {
  usage: MachineUsageReport | undefined;
  usageError: string | undefined;
  diskQuotaGb: number;
}) {
  if (usageError) {
    return (
      <p className="truncate text-xs text-red-600 dark:text-red-400">
        {`Couldn't load usage: ${usageError.split("\n")[0]}`}
      </p>
    );
  }
  if (!usage) {
    return <p className="text-muted-foreground">Loading usage...</p>;
  }
  const percent = Math.min(
    100,
    Math.round((usage.disk_used_bytes / (diskQuotaGb * GIB)) * 100),
  );
  return (
    <div className="space-y-1.5">
      <div className="flex items-center justify-between text-muted-foreground">
        <span>
          {plural(usage.workspace_count, "workspace", "workspaces")} across{" "}
          {plural(usage.repository_count, "repository", "repositories")}
        </span>
        <span>
          {formatBytes(usage.disk_used_bytes)} of {diskQuotaGb} GB
        </span>
      </div>
      <div
        role="progressbar"
        aria-label="Disk usage"
        aria-valuemin={0}
        aria-valuemax={100}
        aria-valuenow={percent}
        className="h-1.5 overflow-hidden rounded-full bg-muted"
      >
        <div
          className={
            percent >= 90 ? "h-full bg-destructive" : "h-full bg-primary"
          }
          style={{ width: `${percent}%` }}
        />
      </div>
    </div>
  );
}

export function CloudWorkspaceCard({
  instanceStatus,
  provisioningStage,
  provisioningError,
  onProvision,
  onWake,
  onRepair,
  onDelete,
  onOpenRepositories,
  usage,
  usageError,
}: CloudWorkspaceCardProps) {
  const [submitting, setSubmitting] = useState(false);
  const [confirmingDelete, setConfirmingDelete] = useState(false);

  const instanceRecord = instanceStatus?.instance ?? null;
  // A deleted instance keeps its provider resource id, but there is nothing
  // left to manage: show the create flow instead.
  const instance =
    instanceRecord?.provider_resource_id && instanceRecord.status !== "deleted"
      ? instanceRecord
      : null;
  const creationRetry =
    instanceRecord?.status === "failed" && !instanceRecord.provider_resource_id;
  const endpoint = instanceStatus?.endpoint ?? null;
  const creating = submitting && !instance;
  // The server refuses ensure/wake/reprovision without Pro. That is an
  // upsell, not a failure.
  const proRequired =
    provisioningError !== undefined && isProRequiredError(provisioningError);
  const errorSummary = proRequired
    ? undefined
    : provisioningError?.split("\n")[0];

  const run = async (action: () => Promise<void>) => {
    setSubmitting(true);
    try {
      await action();
    } finally {
      setSubmitting(false);
    }
  };

  const handleDeleteConfirmed = async () => {
    setConfirmingDelete(false);
    await run(onDelete);
  };

  return (
    <div className="rounded-lg border border-border p-4">
      <div className="flex items-center gap-2">
        <Cloud className="h-4 w-4" />
        <span className="font-medium">Cloud workspace</span>
        {instance && (
          <TooltipProvider>
            <div className="ml-auto flex items-center gap-1">
              {REPAIRABLE_STATES.has(instance.status) && (
                <Tooltip>
                  <TooltipTrigger asChild>
                    <Button
                      size="sm"
                      variant="outline"
                      className="h-7 gap-1.5"
                      disabled={submitting}
                      onClick={() => void run(onRepair)}
                    >
                      <Wrench className="h-3.5 w-3.5" />
                      Repair
                    </Button>
                  </TooltipTrigger>
                  <TooltipContent side="bottom">
                    Re-run Treq setup on this cloud workspace. Files and
                    repositories are kept.
                  </TooltipContent>
                </Tooltip>
              )}
              <Tooltip>
                <TooltipTrigger asChild>
                  <Button
                    size="icon"
                    variant="ghost"
                    aria-label="Delete cloud workspace"
                    className="h-7 w-7 text-muted-foreground hover:text-destructive"
                    disabled={submitting}
                    onClick={() => setConfirmingDelete(true)}
                  >
                    <Trash2 className="h-4 w-4" />
                  </Button>
                </TooltipTrigger>
                <TooltipContent side="bottom">
                  Delete this cloud workspace and all data on it
                </TooltipContent>
              </Tooltip>
            </div>
          </TooltipProvider>
        )}
      </div>

      {instance ? (
        <div className="mt-3 space-y-3 text-sm">
          <div>
            <div className="font-medium">{STAGE_LABELS[instance.status]}</div>
            {endpoint && (
              <p className="text-muted-foreground">
                {endpoint.username}@{endpoint.hostname}:{endpoint.port}
              </p>
            )}
          </div>
          {instance.status === "ready" && (
            <CloudWorkspaceUsage
              usage={usage}
              usageError={usageError}
              diskQuotaGb={instance.disk_quota_gb}
            />
          )}
          {(instance.status === "ready" && endpoint && onOpenRepositories) ||
          instance.status === "suspended" ||
          instance.status === "waking" ? (
            <div className="flex flex-wrap gap-2">
              {instance.status === "ready" &&
                endpoint &&
                onOpenRepositories && (
                  <Button size="sm" onClick={onOpenRepositories}>
                    Open repositories
                  </Button>
                )}
              {(instance.status === "suspended" ||
                instance.status === "waking") && (
                <Button
                  size="sm"
                  disabled={submitting}
                  onClick={() => void run(onWake)}
                >
                  Wake
                </Button>
              )}
            </div>
          ) : null}
        </div>
      ) : (
        <div className="mt-2 space-y-3">
          <p className="text-sm text-muted-foreground">
            Treq creates one persistent cloud workspace for your account, with
            Treq, JJ, Git, and agents installed. Location, CPU, memory, and
            storage are managed for you.
          </p>
          <Button
            size="sm"
            className="w-full gap-2"
            disabled={submitting || Boolean(provisioningStage)}
            onClick={() => void run(onProvision)}
          >
            {(creating || provisioningStage) && (
              <Loader2 className="h-3.5 w-3.5 animate-spin" />
            )}
            {creating || provisioningStage
              ? "Creating..."
              : creationRetry
                ? "Retry cloud workspace creation"
                : "Create cloud workspace"}
          </Button>
          {provisioningStage && (
            <p className="text-sm text-muted-foreground">{provisioningStage}</p>
          )}
        </div>
      )}

      {proRequired && provisioningError && (
        <div className="mt-3 flex items-center gap-2 text-sm">
          <span className="rounded bg-green-500/20 px-1.5 py-0.5 text-xs font-semibold text-green-700 dark:text-green-400">
            PRO
          </span>
          <span className="min-w-0 flex-1 text-muted-foreground">
            {proRequiredMessage(provisioningError)}
          </span>
          <Button
            size="sm"
            variant="outline"
            onClick={() => void openUrl(`${WEB_URL}/dashboard`)}
          >
            Upgrade to Pro
          </Button>
        </div>
      )}

      {errorSummary && (
        <p
          role="alert"
          title={provisioningError}
          className="mt-3 truncate text-xs text-red-600 dark:text-red-400"
        >
          {errorSummary}
        </p>
      )}

      <AlertDialog open={confirmingDelete} onOpenChange={setConfirmingDelete}>
        <AlertDialogContent>
          <AlertDialogHeader>
            <AlertDialogTitle>Delete cloud workspace?</AlertDialogTitle>
            <AlertDialogDescription>
              This permanently deletes the cloud machine and all data on it,
              including cloned repositories, uncommitted changes, and agent
              sessions. This cannot be undone.
            </AlertDialogDescription>
          </AlertDialogHeader>
          <AlertDialogFooter>
            <AlertDialogCancel>Cancel</AlertDialogCancel>
            <AlertDialogAction
              className="bg-destructive text-destructive-foreground hover:bg-destructive/90"
              onClick={() => void handleDeleteConfirmed()}
            >
              Delete
            </AlertDialogAction>
          </AlertDialogFooter>
        </AlertDialogContent>
      </AlertDialog>
    </div>
  );
}
