export const DEFAULT_FETCH_TIMEOUT_MS = 15_000;

/**
 * `fetch` that aborts after `timeoutMs`. A caller-supplied `init.signal` still
 * aborts the request too. `fetchImpl` exists so tests can inject a stub.
 */
export function fetchWithTimeout(
  input: Parameters<typeof fetch>[0],
  init: RequestInit = {},
  timeoutMs: number = DEFAULT_FETCH_TIMEOUT_MS,
  fetchImpl: typeof fetch = fetch,
): Promise<Response> {
  const timeout = AbortSignal.timeout(timeoutMs);
  const signal = init.signal ? AbortSignal.any([init.signal, timeout]) : timeout;
  return fetchImpl(input, { ...init, signal });
}
