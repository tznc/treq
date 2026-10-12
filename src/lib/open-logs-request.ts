import { create } from "zustand";

// A request from outside the workspace view (the PR detail panel) to show a
// workspace's Logs tab once that workspace is open. ShowWorkspace consumes it.
export const useOpenLogsRequest = create<{ workspaceId: number | null }>(
  () => ({ workspaceId: null }),
);

export function requestOpenWorkspaceLogs(workspaceId: number) {
  useOpenLogsRequest.setState({ workspaceId });
}

export function clearOpenWorkspaceLogsRequest() {
  useOpenLogsRequest.setState({ workspaceId: null });
}
