import { create } from "zustand";

// Requests from outside the workspace view to show its Logs tab. ShowWorkspace
// consumes them:
// - `workspaceId`: the PR detail panel asks for a workspace's check logs once
//   that workspace is open.
// - `system`: Help > View Logs asks for the System source in whichever
//   workspace (or home repo) is open.
export const useOpenLogsRequest = create<{
  workspaceId: number | null;
  system: boolean;
}>(() => ({ workspaceId: null, system: false }));

export function requestOpenWorkspaceLogs(workspaceId: number) {
  useOpenLogsRequest.setState({ workspaceId });
}

export function clearOpenWorkspaceLogsRequest() {
  useOpenLogsRequest.setState({ workspaceId: null });
}

export function requestOpenSystemLogs() {
  useOpenLogsRequest.setState({ system: true });
}

export function clearOpenSystemLogsRequest() {
  useOpenLogsRequest.setState({ system: false });
}
