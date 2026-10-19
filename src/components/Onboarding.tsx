import { FolderOpen, Server } from "lucide-react";
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
}

/**
 * Draws the onboarding screen in GPUI, which runs the prerequisite check and
 * opens repositories through the backend itself. Only the remote dialog is
 * still React's.
 */
function useGpuiOnboarding(onOpenRemoteSsh?: () => Promise<void>) {
  useGpuiScreen({ kind: "onboarding" }, (action) => {
    if (action.kind === "openRemote") void onOpenRemoteSsh?.();
  });
}

export const Onboarding: React.FC<OnboardingProps> = ({
  onOpenRepo,
  onOpenRemoteSsh,
  remoteLabel = "Open via SSH",
}) => {
  useGpuiOnboarding(onOpenRemoteSsh);
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
