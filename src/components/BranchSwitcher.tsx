import { ArrowRight, Check, GitBranch } from "lucide-react";
import { listRepoBranches, switchRepoBranch } from "../lib/api";
import { useEffect, useState } from "react";
import { Command } from "cmdk";

// Type definition - Git API removed, needs JJ equivalent
interface BranchListItem {
  isCurrent: boolean;
  isRemote: boolean;
  name: string;
}

interface BranchSwitcherProps {
  open: boolean;
  onOpenChange: (open: boolean) => void;
  repoPath: string;
  onBranchChanged?: (branchName: string) => void;
}

export const BranchSwitcher: React.FC<BranchSwitcherProps> = ({
  open,
  onOpenChange,
  repoPath,
  onBranchChanged,
}) => {
  const [branches, setBranches] = useState<BranchListItem[]>([]);
  const [loading, setLoading] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [switching, setSwitching] = useState(false);

  useEffect(() => {
    if (open) {
      loadBranches();
    }
  }, [open, repoPath]);

  const loadBranches = async () => {
    setLoading(true);
    setError(null);
    try {
      const jjBranches = await listRepoBranches(repoPath);
      const branchList: BranchListItem[] = jjBranches.map((branch) => ({
        isCurrent: branch.is_current,
        isRemote: branch.name.includes("@"), // JJ remote refs contain @
        name: branch.name,
      }));
      setBranches(branchList);
    } catch (err) {
      setError(err instanceof Error ? err.message : String(err));
    } finally {
      setLoading(false);
    }
  };

  const handleSelectBranch = async (branch: BranchListItem) => {
    if (branch.isCurrent) {
      onOpenChange(false);
      return;
    }

    setSwitching(true);
    setError(null);

    try {
      await switchRepoBranch(repoPath, branch.name);
      onBranchChanged?.(branch.name);
      onOpenChange(false);
    } catch (err) {
      setError(err instanceof Error ? err.message : String(err));
    } finally {
      setSwitching(false);
    }
  };

  const groupedBranches = {
    current: branches.filter((branch) => branch.isCurrent),
    local: branches.filter((branch) => !branch.isCurrent && !branch.isRemote),
    remote: branches.filter((branch) => !branch.isCurrent && branch.isRemote),
  };

  return (
    <Command.Dialog
      open={open}
      onOpenChange={onOpenChange}
      label="Switch Branch"
    >
      <span className="sr-only">
        <h2>Switch Branch</h2>
        <p>Switch branch</p>
      </span>
      <div
        data-testid="modal"
        className="bg-popover text-popover-foreground rounded-xl border border-border/50 shadow-2xl w-[40vw] max-w-none overflow-hidden"
      >
        {/* Search Input */}
        <div className="flex items-center border-b border-border px-3">
          <GitBranch className="w-4 h-4 text-muted-foreground mr-2" />
          <Command.Input
            placeholder="Search branches..."
            className="border-0 focus-visible:ring-0 focus-visible:ring-offset-0 h-12 flex-1 bg-transparent outline-none text-sm placeholder:text-muted-foreground"
            disabled={switching}
          />
        </div>

        {/* Results List */}
        <Command.List className="max-h-[400px] overflow-y-auto py-2">
          {loading && (
            <div className="px-4 py-8 text-center text-muted-foreground text-sm">
              Loading branches...
            </div>
          )}

          {error && (
            <div className="px-4 py-3 text-center text-destructive text-sm border-b border-border">
              Error: {error}
            </div>
          )}

          <Command.Empty>
            <div className="px-4 py-8 text-center text-muted-foreground text-sm">
              No branches found
            </div>
          </Command.Empty>

          {!loading && !error && (
            <>
              {/* Current Branch */}
              {groupedBranches.current.length > 0 && (
                <Command.Group heading="Current Branch">
                  {groupedBranches.current.map((branch) => (
                    <Command.Item
                      key={branch.name}
                      value={branch.name}
                      onSelect={() => handleSelectBranch(branch)}
                      disabled={switching}
                      className="px-3 py-2 flex items-center gap-3 cursor-pointer aria-selected:bg-accent aria-selected:text-accent-foreground data-[disabled='true']:opacity-50 data-[disabled='true']:pointer-events-none"
                    >
                      <Check className="w-4 h-4 text-green-500" />
                      <div className="flex-1 min-w-0">
                        <div className="truncate text-sm font-medium">
                          {branch.name}
                        </div>
                      </div>
                      <span className="text-sm text-muted-foreground">
                        current
                      </span>
                    </Command.Item>
                  ))}
                </Command.Group>
              )}

              {/* Local Branches */}
              {groupedBranches.local.length > 0 && (
                <Command.Group heading="Local Branches">
                  {groupedBranches.local.map((branch) => (
                    <Command.Item
                      key={branch.name}
                      value={branch.name}
                      onSelect={() => handleSelectBranch(branch)}
                      disabled={switching}
                      className="px-3 py-2 flex items-center gap-3 cursor-pointer aria-selected:bg-accent aria-selected:text-accent-foreground data-[disabled='true']:opacity-50 data-[disabled='true']:pointer-events-none"
                    >
                      <GitBranch className="w-4 h-4 text-muted-foreground" />
                      <div className="flex-1 min-w-0">
                        <div className="truncate text-sm">{branch.name}</div>
                      </div>
                    </Command.Item>
                  ))}
                </Command.Group>
              )}

              {/* Remote Branches */}
              {groupedBranches.remote.length > 0 && (
                <Command.Group heading="Remote Branches">
                  {groupedBranches.remote.map((branch) => (
                    <Command.Item
                      key={branch.name}
                      value={branch.name}
                      onSelect={() => handleSelectBranch(branch)}
                      disabled={switching}
                      className="px-3 py-2 flex items-center gap-3 cursor-pointer aria-selected:bg-accent aria-selected:text-accent-foreground data-[disabled='true']:opacity-50 data-[disabled='true']:pointer-events-none"
                    >
                      <ArrowRight className="w-4 h-4 text-muted-foreground" />
                      <div className="flex-1 min-w-0">
                        <div className="truncate text-sm">{branch.name}</div>
                      </div>
                      <span className="text-sm text-muted-foreground">
                        remote
                      </span>
                    </Command.Item>
                  ))}
                </Command.Group>
              )}
            </>
          )}
        </Command.List>

        {/* Footer with keyboard hints */}
        <div className="border-t border-border px-3 py-2 flex items-center justify-between text-sm text-muted-foreground">
          <div className="flex items-center gap-3">
            <span>
              <kbd className="px-1.5 py-0.5 bg-muted rounded text-xs">↑↓</kbd>{" "}
              Navigate
            </span>
            <span>
              <kbd className="px-1.5 py-0.5 bg-muted rounded text-xs">↵</kbd>{" "}
              Switch
            </span>
            <span>
              <kbd className="px-1.5 py-0.5 bg-muted rounded text-xs">Esc</kbd>{" "}
              Close
            </span>
          </div>
          {switching && <span className="text-sm">Switching...</span>}
        </div>
      </div>
    </Command.Dialog>
  );
};
