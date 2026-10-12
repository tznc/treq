import { Settings, type LucideIcon } from "lucide-react";
import { Tooltip, TooltipContent, TooltipTrigger } from "./ui/tooltip";

function SidebarIconButton({
  label,
  testId,
  active,
  icon: Icon,
  onClick,
}: {
  label: string;
  testId?: string;
  active: boolean;
  icon: LucideIcon;
  onClick: () => void;
}) {
  return (
    <Tooltip>
      <TooltipTrigger asChild>
        <button
          type="button"
          data-testid={testId}
          onClick={onClick}
          className={`h-9 w-9 rounded-lg hover:bg-muted flex items-center justify-center transition-colors border border-border ${
            active ? "bg-primary/20" : "bg-muted/50"
          }`}
          aria-label={label}
        >
          <Icon
            className={`w-4 h-4 ${
              active ? "text-primary" : "text-muted-foreground"
            }`}
          />
        </button>
      </TooltipTrigger>
      <TooltipContent side="bottom">{label}</TooltipContent>
    </Tooltip>
  );
}

export function WorkspaceSidebarHeaderActions({
  currentPage,
  openSettings,
}: {
  currentPage?: string;
  openSettings?: (tab?: string) => void;
}) {
  return (
    <>
      {openSettings && (
        <SidebarIconButton
          label="Settings"
          active={currentPage === "settings"}
          icon={Settings}
          onClick={() => openSettings("application")}
        />
      )}
    </>
  );
}
