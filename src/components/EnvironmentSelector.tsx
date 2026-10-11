import { Box, Check, Laptop, Loader2, Play } from "lucide-react";
import type { RepoEnvironment } from "../hooks/useRepoEnvironment";
import type { DevcontainerStatus } from "../lib/api-devcontainer";
import { cn } from "../lib/utils";
import { Button } from "./ui/button";
import {
  DropdownMenu,
  DropdownMenuContent,
  DropdownMenuItem,
  DropdownMenuLabel,
  DropdownMenuSeparator,
  DropdownMenuTrigger,
} from "./ui/dropdown-menu";

interface EnvironmentSelectorProps {
  env: RepoEnvironment;
  className?: string;
}

function devcontainerDetail(
  status: DevcontainerStatus | undefined,
  starting: boolean,
): string {
  if (!status) return "Checking…";
  if (!status.configured) return "No .devcontainer config in this repo";
  if (!status.cliAvailable) return "Install the Dev Containers CLI";
  if (starting) return "Starting…";
  return status.running ? "Running" : "Stopped";
}

/**
 * Picks where new terminals and agent sessions in a repository run: on this
 * machine, or in the repository's devcontainer.
 */
export function EnvironmentSelector({
  env,
  className,
}: EnvironmentSelectorProps) {
  const { environment, status, starting } = env;
  const isDevcontainer = environment === "devcontainer";
  const devcontainerUsable = !!status?.configured && !!status.cliAvailable;
  const label = isDevcontainer ? "Devcontainer" : "Local";
  const Icon = isDevcontainer ? Box : Laptop;

  return (
    <DropdownMenu>
      <DropdownMenuTrigger asChild>
        <Button
          type="button"
          variant="ghost"
          size="sm"
          className={cn("h-7 gap-1.5 px-2 text-xs", className)}
          aria-label={`Environment: ${label}`}
        >
          <Icon className="h-3.5 w-3.5" aria-hidden="true" />
          {label}
          {isDevcontainer &&
            (starting ? (
              <Loader2
                className="h-3 w-3 animate-spin text-muted-foreground"
                aria-label="Starting"
              />
            ) : (
              <span
                className={cn(
                  "h-1.5 w-1.5 rounded-full",
                  status?.running ? "bg-green-500" : "bg-muted-foreground/50",
                )}
                aria-label={status?.running ? "Running" : "Stopped"}
              />
            ))}
        </Button>
      </DropdownMenuTrigger>
      <DropdownMenuContent align="end" sideOffset={4} className="w-64">
        <DropdownMenuLabel className="text-xs font-normal text-muted-foreground">
          New terminals and agents run in
        </DropdownMenuLabel>
        <EnvironmentItem
          icon={Laptop}
          title="Local"
          detail="This machine"
          selected={!isDevcontainer}
          onSelect={() => void env.setEnvironment("local")}
        />
        <EnvironmentItem
          icon={Box}
          title="Devcontainer"
          detail={devcontainerDetail(status, starting)}
          selected={isDevcontainer}
          disabled={!devcontainerUsable}
          onSelect={() => void env.setEnvironment("devcontainer")}
        />
        {isDevcontainer && devcontainerUsable && !status?.running && (
          <>
            <DropdownMenuSeparator />
            <DropdownMenuItem
              disabled={starting}
              onClick={() => void env.startDevcontainer()}
            >
              <Play className="h-3.5 w-3.5" aria-hidden="true" />
              Start devcontainer
            </DropdownMenuItem>
          </>
        )}
      </DropdownMenuContent>
    </DropdownMenu>
  );
}

function EnvironmentItem({
  icon: Icon,
  title,
  detail,
  selected,
  disabled,
  onSelect,
}: {
  icon: typeof Laptop;
  title: string;
  detail: string;
  selected: boolean;
  disabled?: boolean;
  onSelect: () => void;
}) {
  return (
    <DropdownMenuItem
      disabled={disabled}
      onClick={onSelect}
      className="items-start gap-2"
    >
      <Icon className="mt-0.5 h-3.5 w-3.5 shrink-0" aria-hidden="true" />
      <div className="flex min-w-0 flex-1 flex-col gap-0.5">
        <span className="font-medium">{title}</span>
        <span className="text-xs text-muted-foreground">{detail}</span>
      </div>
      {selected && (
        <Check className="mt-0.5 h-3.5 w-3.5 shrink-0" aria-label="Selected" />
      )}
    </DropdownMenuItem>
  );
}
