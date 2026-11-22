/**
 * Polls `probe` until it returns a value, for state another process changes
 * on its own time (Mailpit receiving an email, an Edge Function settling an
 * instance). Jest has no `expect.poll`, so this is service-qa's one
 * poll-with-deadline; the no-wait-for-timeout rule exempts this file only.
 * Throws once `timeout` passes without a value.
 */
export async function pollUntil<T>(
  what: string,
  probe: () => Promise<T | undefined>,
  { timeout = 10_000, interval = 250 }: { timeout?: number; interval?: number } = {},
): Promise<T> {
  const deadline = Date.now() + timeout;
  for (;;) {
    const value = await probe();
    if (value !== undefined) return value;
    if (Date.now() >= deadline) throw new Error(`timed out after ${timeout} ms waiting for ${what}`);
    await new Promise((resolve) => setTimeout(resolve, interval));
  }
}
