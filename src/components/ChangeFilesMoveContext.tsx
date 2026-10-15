import * as React from "react";
import type { ChangeFilesMoveRequest } from "../lib/change-file-drag";

/** A workspace, or the home repository, that changed files can move to. */
export interface ChangeFilesMoveTarget {
  /** A workspace branch_name, or HOME_MOVE_ENDPOINT for the home repository. */
  branch: string;
  label: string;
}

interface ChangeFilesMoveValue {
  targets: ChangeFilesMoveTarget[];
  /** Opens the move confirmation for `request`. */
  requestMove: (request: ChangeFilesMoveRequest) => void;
}

const ChangeFilesMoveContext = React.createContext<ChangeFilesMoveValue | null>(
  null,
);

export const ChangeFilesMoveProvider = ChangeFilesMoveContext.Provider;

/** Where the Review tab's "Move to workspace" menu can send files, if anywhere. */
export function useChangeFilesMove(): ChangeFilesMoveValue | null {
  return React.useContext(ChangeFilesMoveContext);
}
