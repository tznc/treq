import {
  refreshRemoteRepository,
  useRemoteMutationFeedback,
} from "../../lib/remote-mutation-ui";
import { Button } from "../ui/button";
import {
  Dialog,
  DialogContent,
  DialogDescription,
  DialogHeader,
  DialogTitle,
} from "../ui/dialog";

export function RemoteAmbiguousMutationDialog() {
  const reason = useRemoteMutationFeedback((s) => s.ambiguousReason);
  const clearAmbiguous = useRemoteMutationFeedback((s) => s.clearAmbiguous);

  return (
    <Dialog
      open={Boolean(reason)}
      onOpenChange={(open) => !open && clearAmbiguous()}
    >
      <DialogContent>
        <DialogHeader>
          <DialogTitle>Remote change could not be verified</DialogTitle>
          <DialogDescription>
            A network interruption happened while a mutation was in flight. Treq
            did not retry automatically because the remote state is ambiguous.
            Refresh to check whether the change landed before trying again.
          </DialogDescription>
        </DialogHeader>
        <p className="text-sm" data-testid="remote-ambiguous-reason">
          {reason}
        </p>
        <div className="flex justify-end gap-2">
          <Button type="button" variant="outline" onClick={clearAmbiguous}>
            Dismiss
          </Button>
          <Button
            type="button"
            onClick={() => {
              refreshRemoteRepository();
              clearAmbiguous();
            }}
          >
            Refresh
          </Button>
        </div>
      </DialogContent>
    </Dialog>
  );
}
