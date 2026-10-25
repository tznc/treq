import { functionErrorMessage } from "./billing";
import { supabase } from "./supabase";

/** Whether `typed` confirms deleting the account of `email`. */
export function confirmsAccountDeletion(typed: string, email: string): boolean {
  return email !== "" && typed.trim().toLowerCase() === email.toLowerCase();
}

/**
 * Asks delete-account to delete the signed-in account. It cancels the
 * subscription and destroys the Treq Cloud machines first. A refusal, such
 * as being the last owner of an organization, throws the server's sentence.
 */
export async function deleteAccount(): Promise<void> {
  const { error } = await supabase.functions.invoke("delete-account", {
    body: {},
  });
  if (error) {
    throw new Error(
      await functionErrorMessage(error, "Your account could not be deleted."),
    );
  }
}
