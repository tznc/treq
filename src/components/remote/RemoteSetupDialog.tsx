import { useEffect, useState } from "react";
import {
  Dialog,
  DialogContent,
  DialogDescription,
  DialogHeader,
  DialogTitle,
} from "../ui/dialog";
import type {
  InstanceStatusResponse,
  MachineUsageReport,
} from "../../lib/api-types-remote";
import { Button } from "../ui/button";
import { CloudWorkspaceCard } from "./CloudWorkspaceCard";
import { RemoteUserManagedSetupPanel } from "./RemoteUserManagedSetupPanel";
import type {
  LocalKeyIdentity,
  UserManagedFormValues,
} from "./remoteSetupLabels";

export type { LocalKeyIdentity, UserManagedFormValues };

export type RemoteSetupMode = "choice" | "managed" | "user_managed";

export interface RemoteSetupDialogProps {
  open: boolean;
  onOpenChange: (open: boolean) => void;

  /** Autocomplete-only suggestions read from `~/.ssh/config` (Goal 15 / Phase 1). Selecting one only fills the field. */
  sshConfigAliasSuggestions: string[];

  instanceStatus: InstanceStatusResponse | null;
  provisioningStage?: string;
  provisioningError?: string;
  cloudUsage?: MachineUsageReport;
  cloudUsageError?: string;

  onProvisionManaged: () => Promise<void>;
  onWake: () => Promise<void>;
  onReprovision: () => Promise<void>;
  onDeleteInstance: () => Promise<void>;
  onRevokeKey: (keyReference: string) => Promise<void>;

  onRegisterUserManaged: (values: UserManagedFormValues) => Promise<void>;
  /** Open repositories on the existing managed instance without provisioning another. */
  onOpenManagedRepositories?: () => void;
  /**
   * Whether the "Your own VM" path is offered. When false (the `remoteSsh`
   * preview is off) the dialog opens straight on the managed cloud workspace.
   */
  allowUserManaged?: boolean;
}

/**
 * Remote setup flow: the two-choice entry point plus the managed and
 * user-managed configuration screens (PRD "UI requirements" / "Remote
 * setup"). Replaces the old bare host+path dialog. The two configuration
 * screens live in `CloudWorkspaceCard` (also shown inline on the account
 * settings page) and `RemoteUserManagedSetupPanel`; this component only owns
 * which screen is showing.
 */
export function RemoteSetupDialog({
  open,
  onOpenChange,
  sshConfigAliasSuggestions,
  instanceStatus,
  provisioningStage,
  provisioningError,
  cloudUsage,
  cloudUsageError,
  onProvisionManaged,
  onWake,
  onReprovision,
  onDeleteInstance,
  onRegisterUserManaged,
  onOpenManagedRepositories,
  allowUserManaged = true,
}: RemoteSetupDialogProps) {
  const initialMode: RemoteSetupMode = allowUserManaged ? "choice" : "managed";
  const [mode, setMode] = useState<RemoteSetupMode>(initialMode);

  useEffect(() => {
    if (open) setMode(initialMode);
  }, [open, initialMode]);

  return (
    <Dialog open={open} onOpenChange={onOpenChange}>
      <DialogContent className="max-w-xl">
        {mode === "choice" && (
          <>
            <DialogHeader>
              <DialogTitle>Connect a remote repository</DialogTitle>
              <DialogDescription>
                Choose how the remote machine is managed.
              </DialogDescription>
            </DialogHeader>
            <div className="mt-4 grid grid-cols-1 gap-3 sm:grid-cols-2">
              <button
                type="button"
                className="rounded-lg border border-border/60 p-4 text-left hover:border-primary/60 hover:bg-muted/40"
                onClick={() => setMode("managed")}
              >
                <div className="font-medium">Treq-managed cloud workspace</div>
                <p className="mt-1 text-sm text-muted-foreground">
                  Treq provisions and maintains one persistent cloud workspace
                  for your account.
                </p>
              </button>
              <button
                type="button"
                className="rounded-lg border border-border/60 p-4 text-left hover:border-primary/60 hover:bg-muted/40"
                onClick={() => setMode("user_managed")}
              >
                <div className="font-medium">Your own VM</div>
                <p className="mt-1 text-sm text-muted-foreground">
                  Connect to a machine you already run. You install Treq, JJ,
                  Git, and agents there yourself.
                </p>
              </button>
            </div>
          </>
        )}

        {mode === "managed" && (
          <>
            <DialogHeader>
              <DialogTitle>Treq-managed cloud workspace</DialogTitle>
              <DialogDescription>
                Treq manages the machine lifecycle. Commands run over managed
                SSH with short-lived certificates.
              </DialogDescription>
            </DialogHeader>
            <div className="mt-4">
              <CloudWorkspaceCard
                instanceStatus={instanceStatus}
                provisioningStage={provisioningStage}
                provisioningError={provisioningError}
                onProvision={onProvisionManaged}
                onWake={onWake}
                onRepair={onReprovision}
                onDelete={onDeleteInstance}
                onOpenRepositories={onOpenManagedRepositories}
                usage={cloudUsage}
                usageError={cloudUsageError}
              />
            </div>
            {allowUserManaged && (
              <div className="mt-6">
                <Button variant="ghost" onClick={() => setMode("choice")}>
                  Back
                </Button>
              </div>
            )}
          </>
        )}

        {mode === "user_managed" && (
          <>
            <DialogHeader>
              <DialogTitle>Your own VM</DialogTitle>
              <DialogDescription>
                Enter the connection details exactly - Treq never infers trust
                from your local SSH configuration.
              </DialogDescription>
            </DialogHeader>
            <RemoteUserManagedSetupPanel
              sshConfigAliasSuggestions={sshConfigAliasSuggestions}
              onBack={() => setMode("choice")}
              onRegisterUserManaged={onRegisterUserManaged}
            />
          </>
        )}
      </DialogContent>
    </Dialog>
  );
}
