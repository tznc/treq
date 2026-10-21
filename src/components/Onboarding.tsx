import { FolderOpen, Server } from "lucide-react";
import type { GpuiRemoteRepoForm } from "../lib/gpui-remote-types";
import { useGpuiScreen } from "../lib/gpui-screen";
import { PrerequisiteChecklist } from "./PrerequisiteChecklist";
import { Button } from "./ui/button";
import {
  Card,
  CardContent,
  CardDescription,
  CardHeader,
  CardTitle,
} from "./ui/card";

interface OnboardingProps {
  onOpenRepo: () => Promise<void>;
  onOpenRemoteSsh?: () => Promise<void>;
  remoteLabel?: string;
  /** Dashboard's explicit-endpoint form, which GPUI draws under the card. */
  gpuiRemoteRepo?: {
    form: GpuiRemoteRepoForm;
    press: (id: string) => void;
    field: (name: string, value: unknown) => void;
  } | null;
}

/**
 * Draws the onboarding screen in GPUI, which runs the prerequisite check and
 * opens repositories through the backend itself. Only the remote dialog is
 * still React's.
 */
function useGpuiOnboarding(
  onOpenRemoteSsh: (() => Promise<void>) | undefined,
  remoteRepo: OnboardingProps["gpuiRemoteRepo"],
) {
  useGpuiScreen(
    {
      kind: "onboarding",
      ...(remoteRepo?.form ? { remoteRepo: remoteRepo.form } : {}),
    },
    (action) => {
      if (action.kind === "openRemote") void onOpenRemoteSsh?.();
      if (action.kind === "press") remoteRepo?.press(action.id);
      if (action.kind === "field")
        remoteRepo?.field(action.field, action.value);
    },
  );
}

export const Onboarding: React.FC<OnboardingProps> = ({
  onOpenRepo,
  onOpenRemoteSsh,
  remoteLabel = "Open via SSH",
  gpuiRemoteRepo,
}) => {
  useGpuiOnboarding(onOpenRemoteSsh, gpuiRemoteRepo);
  return (
    <div className="flex h-screen items-center justify-center bg-background mx-auto">
      <Card className="w-96">
        <CardHeader className="text-center">
          <div className="flex justify-center mb-2">
            <FolderOpen className="w-10 h-10 text-muted-foreground" />
          </div>
          <CardTitle>Open a Repository</CardTitle>
          <CardDescription>
            Select a folder containing a Git repository to get started.
          </CardDescription>
        </CardHeader>
        <CardContent className="flex flex-col items-center gap-3">
          <Button onClick={onOpenRepo} className="w-full">
            Open Repository
          </Button>
          {onOpenRemoteSsh && (
            <Button
              onClick={onOpenRemoteSsh}
              variant="outline"
              className="w-full"
            >
              <Server className="w-4 h-4 mr-2" />
              {remoteLabel}
            </Button>
          )}
        </CardContent>
        <CardContent className="border-t border-border/50 pt-4">
          <PrerequisiteChecklist />
        </CardContent>
      </Card>
    </div>
  );
};
