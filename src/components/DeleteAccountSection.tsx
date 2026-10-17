import { Loader2, Trash2 } from "lucide-react";
import { useState } from "react";
import { useAuthStore } from "../stores/authStore";
import {
  AlertDialog,
  AlertDialogCancel,
  AlertDialogContent,
  AlertDialogDescription,
  AlertDialogFooter,
  AlertDialogHeader,
  AlertDialogTitle,
} from "./ui/alert-dialog";
import { Button } from "./ui/button";
import { Input } from "./ui/input";
import { Label } from "./ui/label";

/**
 * Deletes the signed-in Treq account after the user types their email.
 * On success the auth store drops the local session, so the account screen
 * falls back to the sign-in prompt.
 */
export function DeleteAccountSection({ email }: { email: string }) {
  const deleteAccount = useAuthStore((s) => s.deleteAccount);
  const [open, setOpen] = useState(false);
  const [typed, setTyped] = useState("");
  const [deleting, setDeleting] = useState(false);
  const [error, setError] = useState<string | null>(null);

  const matches =
    email.length > 0 && typed.trim().toLowerCase() === email.toLowerCase();

  const handleOpenChange = (next: boolean) => {
    if (deleting) return;
    setOpen(next);
    if (!next) {
      setTyped("");
      setError(null);
    }
  };

  const handleDelete = async () => {
    setDeleting(true);
    setError(null);
    try {
      await deleteAccount();
    } catch (err) {
      setError(err instanceof Error ? err.message : "Account deletion failed");
      setDeleting(false);
    }
  };

  return (
    <div className="border border-destructive/40 rounded-lg p-4">
      <div className="font-medium">Delete account</div>
      <p className="text-sm text-muted-foreground mt-1">
        Permanently deletes your Treq account, cancels any subscription
        immediately, and deletes your cloud workspace.
      </p>
      <Button
        variant="outline"
        size="sm"
        className="mt-3 gap-2 text-destructive hover:text-destructive"
        onClick={() => setOpen(true)}
      >
        <Trash2 className="w-4 h-4" />
        Delete Account
      </Button>

      <AlertDialog open={open} onOpenChange={handleOpenChange}>
        <AlertDialogContent role="alertdialog">
          <AlertDialogHeader>
            <AlertDialogTitle>Delete your Treq account?</AlertDialogTitle>
            <AlertDialogDescription>
              This cannot be undone. Your subscription is canceled immediately
              without a refund for the remaining period. Your cloud workspace
              and everything on it is deleted. Treq disconnects Linear and
              forgets your GitHub App installations. Organizations you are the
              only member of are deleted. Billing records are kept for tax and
              accounting.
            </AlertDialogDescription>
          </AlertDialogHeader>
          <div className="space-y-2 pt-2">
            <Label htmlFor="delete-account-confirm">
              Type your email ({email}) to confirm
            </Label>
            <Input
              id="delete-account-confirm"
              value={typed}
              onChange={(event) => setTyped(event.target.value)}
              autoComplete="off"
              disabled={deleting}
            />
            {error && (
              <p role="alert" className="text-sm text-destructive">
                {error}
              </p>
            )}
          </div>
          <AlertDialogFooter>
            <AlertDialogCancel disabled={deleting}>Cancel</AlertDialogCancel>
            <Button
              className="bg-destructive text-destructive-foreground hover:bg-destructive/90 gap-2"
              disabled={!matches || deleting}
              onClick={() => void handleDelete()}
            >
              {deleting && <Loader2 className="w-4 h-4 animate-spin" />}
              Delete Account
            </Button>
          </AlertDialogFooter>
        </AlertDialogContent>
      </AlertDialog>
    </div>
  );
}
