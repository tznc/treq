/** The message of a caught value, for QA notes and logs that need a string. */
export function errorMessage(err: unknown): string {
  return err instanceof Error ? err.message : String(err);
}
