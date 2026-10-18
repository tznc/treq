import { useState } from "react";
import { Check, ChevronDown, GitBranch, Loader2 } from "lucide-react";
import { Command } from "cmdk";
import { Button } from "./ui/button";
import { Popover, PopoverContent, PopoverTrigger } from "./ui/popover";
import { useGpuiPart } from "../lib/gpui-parts";

// Define BranchListItem locally since git API was removed
export interface BranchListItem {
  name: string;
  fullName: string;
  isCurrent: boolean;
}

interface TargetBranchSelectorProps {
  branches: BranchListItem[];
  loading: boolean;
  targetBranch: string | null;
  onSelect: (branch: string) => void;
  onOpenChange?: (open: boolean) => void;
  disabled?: boolean;
}

export const TargetBranchSelector: React.FC<TargetBranchSelectorProps> = ({
  branches,
  loading,
  targetBranch,
  onSelect,
  onOpenChange,
  disabled,
}) => {
  const [open, setOpen] = useState(false);
  const changeOpen = (value: boolean) => {
    setOpen(value);
    onOpenChange?.(value);
  };
  // While it is open, the screen hands the header back to React to draw it.
  useGpuiPart(
    "target",
    { open, branch: targetBranch, loading, disabled: !!disabled || loading },
    { open: () => changeOpen(true) },
  );

  return (
    <Popover open={open} onOpenChange={changeOpen}>
      <PopoverTrigger asChild>
        <Button
          variant="outline"
          size="sm"
          disabled={disabled || loading}
          className="gap-2 max-w-[220px]"
          aria-label="Workspace target"
        >
          {loading ? (
            <Loader2 className="w-4 h-4 animate-spin shrink-0" />
          ) : (
            <GitBranch className="w-4 h-4 shrink-0" />
          )}
          <span
            className="font-mono truncate min-w-0"
            title={targetBranch ?? undefined}
          >
            {targetBranch || "Select..."}
          </span>
          <ChevronDown className="w-4 h-4 opacity-50 shrink-0" />
        </Button>
      </PopoverTrigger>
      <PopoverContent className="w-[300px] p-0" align="start">
        <Command>
          <Command.Input
            placeholder="Search branches..."
            className="border-0 focus-visible:ring-0 focus-visible:ring-offset-0 h-10 px-3"
          />
          <Command.List className="max-h-[300px] overflow-auto">
            {loading ? (
              <div className="px-2 py-4 text-sm text-center text-muted-foreground">
                <Loader2 className="w-4 h-4 animate-spin mx-auto mb-1" />
                Loading branches...
              </div>
            ) : (
              <>
                <Command.Empty>
                  <div className="px-2 py-4 text-sm text-center text-muted-foreground">
                    No branches found
                  </div>
                </Command.Empty>
                {branches.map((branch) => (
                  <Command.Item
                    key={branch.fullName}
                    value={branch.name}
                    onSelect={() => {
                      onSelect(branch.name);
                      setOpen(false);
                    }}
                    className="branch-list-item px-3 py-1.5 flex items-center gap-2 cursor-pointer aria-selected:bg-accent font-mono"
                  >
                    <span
                      className="flex-1 truncate min-w-0"
                      title={branch.name}
                    >
                      {branch.name}
                    </span>
                    {branch.name === targetBranch && (
                      <Check className="w-4 h-4 shrink-0" />
                    )}
                  </Command.Item>
                ))}
              </>
            )}
          </Command.List>
        </Command>
      </PopoverContent>
    </Popover>
  );
};
