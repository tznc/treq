import { ArrowRightLeft, Copy, FolderOpen } from "lucide-react";
import type { ChangeFilesMoveTarget } from "./ChangeFilesMoveContext";
import {
  ContextMenu,
  ContextMenuContent,
  ContextMenuItem,
  ContextMenuSeparator,
  ContextMenuSub,
  ContextMenuSubContent,
  ContextMenuSubTrigger,
  ContextMenuTrigger,
} from "./ui/context-menu";
import { openUrl, revealItemInDir } from "@tauri-apps/plugin-opener";
import { useToast } from "./ui/toast";
import { useEditorAppsStore } from "../stores/editorAppsStore";

interface FileContextMenuProps {
  filePath: string;
  workspacePath: string;
  children: React.ReactNode;
  /** Workspaces the file can move to; leaving it out hides "Move to workspace". */
  moveTargets?: ChangeFilesMoveTarget[];
  onMoveTo?: (target: ChangeFilesMoveTarget) => void;
}

export const FileContextMenu = ({
  filePath,
  workspacePath,
  children,
  moveTargets,
  onMoveTo,
}: FileContextMenuProps) => {
  const { addToast } = useToast();
  const editorApps = useEditorAppsStore();

  const getRelativePath = (fullPath: string): string => {
    if (workspacePath && fullPath.startsWith(`${workspacePath}/`)) {
      return fullPath.slice(workspacePath.length + 1);
    }
    return fullPath;
  };

  const getFullPath = (path: string): string => {
    // If path is already absolute, return it
    if (path.startsWith("/")) return path;
    // Otherwise, join with workspace path
    return `${workspacePath}/${path}`;
  };

  const fullPath = getFullPath(filePath);
  const relativePath = getRelativePath(fullPath);

  return (
    <ContextMenu>
      <ContextMenuTrigger asChild>{children}</ContextMenuTrigger>
      <ContextMenuContent>
        <ContextMenuItem
          data-testid="copy-relative-path"
          onClick={async () => {
            try {
              await navigator.clipboard.writeText(relativePath);
              addToast({
                title: "Copied",
                description: `Copied relative path: ${relativePath}`,
                type: "success",
              });
            } catch (err) {
              addToast({
                title: "Copy Failed",
                description: err instanceof Error ? err.message : String(err),
                type: "error",
              });
            }
          }}
        >
          <Copy className="w-4 h-4 mr-2" />
          Copy relative path
        </ContextMenuItem>

        <ContextMenuItem
          data-testid="copy-full-path"
          onClick={async () => {
            try {
              await navigator.clipboard.writeText(fullPath);
              addToast({
                title: "Copied",
                description: `Copied full path: ${fullPath}`,
                type: "success",
              });
            } catch (err) {
              addToast({
                title: "Copy Failed",
                description: err instanceof Error ? err.message : String(err),
                type: "error",
              });
            }
          }}
        >
          <Copy className="w-4 h-4 mr-2" />
          Copy full path
        </ContextMenuItem>

        <ContextMenuSeparator />

        <ContextMenuSub>
          <ContextMenuSubTrigger>
            <FolderOpen className="w-4 h-4 mr-2" />
            Open in...
          </ContextMenuSubTrigger>
          <ContextMenuSubContent>
            <ContextMenuItem
              onClick={async () => {
                try {
                  await revealItemInDir(fullPath);
                } catch (err) {
                  addToast({
                    title: "Open Failed",
                    description:
                      err instanceof Error ? err.message : String(err),
                    type: "error",
                  });
                }
              }}
            >
              <FolderOpen className="w-4 h-4 mr-2" />
              Open in Finder
            </ContextMenuItem>

            {editorApps.cursor && (
              <ContextMenuItem
                onClick={async () => {
                  try {
                    await openUrl(`cursor://file/${fullPath}`);
                  } catch (err) {
                    addToast({
                      title: "Open Failed",
                      description:
                        err instanceof Error ? err.message : String(err),
                      type: "error",
                    });
                  }
                }}
              >
                Open in Cursor
              </ContextMenuItem>
            )}

            {editorApps.vscode && (
              <ContextMenuItem
                onClick={async () => {
                  try {
                    await openUrl(`vscode://file/${fullPath}`);
                  } catch (err) {
                    addToast({
                      title: "Open Failed",
                      description:
                        err instanceof Error ? err.message : String(err),
                      type: "error",
                    });
                  }
                }}
              >
                Open in VSCode
              </ContextMenuItem>
            )}

            {editorApps.zed && (
              <ContextMenuItem
                onClick={async () => {
                  try {
                    await openUrl(`zed://file/${fullPath}`);
                  } catch (err) {
                    addToast({
                      title: "Open Failed",
                      description:
                        err instanceof Error ? err.message : String(err),
                      type: "error",
                    });
                  }
                }}
              >
                Open in Zed
              </ContextMenuItem>
            )}
          </ContextMenuSubContent>
        </ContextMenuSub>
        {onMoveTo && moveTargets && moveTargets.length > 0 && (
          <>
            <ContextMenuSeparator />
            <ContextMenuSub>
              <ContextMenuSubTrigger data-testid="move-to-workspace">
                <ArrowRightLeft className="w-4 h-4 mr-2" />
                Move to workspace...
              </ContextMenuSubTrigger>
              <ContextMenuSubContent>
                {moveTargets.map((target) => (
                  <ContextMenuItem
                    key={target.branch}
                    onClick={() => onMoveTo(target)}
                  >
                    {target.label}
                  </ContextMenuItem>
                ))}
              </ContextMenuSubContent>
            </ContextMenuSub>
          </>
        )}
      </ContextMenuContent>
    </ContextMenu>
  );
};
