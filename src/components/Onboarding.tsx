import { openUrl } from "@tauri-apps/plugin-opener";
import { FolderOpen, Server } from "lucide-react";
import { useGpuiScreen } from "../lib/gpui-screen";
import {
  PrerequisiteChecklist,
  useGpuiChecklist,
} from "./PrerequisiteChecklist";
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

/** Draws the onboarding screen in GPUI, with the same handlers. */
function useGpuiOnboarding({
  onOpenRepo,
  onOpenRemoteSsh,
  remoteLabel,
}: Required<Pick<OnboardingProps, "onOpenRepo" | "remoteLabel">> &
  Pick<OnboardingProps, "onOpenRemoteSsh">) {
  const { checklist, recheck } = useGpuiChecklist();
  useGpuiScreen(
    {
      kind: "onboarding",
      remoteLabel: onOpenRemoteSsh ? remoteLabel : null,
      checklist,
    },
    (action) => {
      switch (action.kind) {
        case "openRepo":
          void onOpenRepo();
          break;
        case "openRemote":
          void onOpenRemoteSsh?.();
          break;
        case "recheckPrerequisites":
          recheck();
          break;
        case "openUrl":
          void openUrl(action.url);
          break;
      }
    },
  );
}

export const Onboarding: React.FC<OnboardingProps> = ({
  onOpenRepo,
  onOpenRemoteSsh,
  remoteLabel = "Open via SSH",
}) => {
  useGpuiOnboarding({ onOpenRepo, onOpenRemoteSsh, remoteLabel });
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
