/**
 * The message of a caught value, for logs, audit rows and responses that need
 * a string. Rethrow with `{ cause: err }` instead where the stack matters.
 */
export function errorMessage(err: unknown): string {
  return err instanceof Error ? err.message : String(err);
}
