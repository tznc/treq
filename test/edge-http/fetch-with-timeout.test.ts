import { afterEach, describe, expect, it, vi } from "vitest";
import { fetchWithTimeout } from "../../supabase/functions/_shared/http/fetch-with-timeout";

// A fetch that never responds and only settles when its signal aborts.
const hangingFetch = ((_input: unknown, init?: RequestInit) =>
  new Promise<Response>((_resolve, reject) => {
    const signal = init?.signal;
    if (signal?.aborted) return reject(signal.reason);
    signal?.addEventListener("abort", () => reject(signal.reason));
  })) as unknown as typeof fetch;

afterEach(() => {
  vi.useRealTimers();
});

describe("fetchWithTimeout", () => {
  it("passes a successful response through", async () => {
    const fetchImpl = vi.fn(
      async () => new Response("ok"),
    ) as unknown as typeof fetch;
    const res = await fetchWithTimeout(
      "https://example.test",
      { method: "POST" },
      1_000,
      fetchImpl,
    );
    expect(await res.text()).toBe("ok");
    expect(fetchImpl).toHaveBeenCalledWith(
      "https://example.test",
      expect.objectContaining({
        method: "POST",
        signal: expect.any(AbortSignal),
      }),
    );
  });

  it("aborts with a TimeoutError when the request hangs", async () => {
    vi.useFakeTimers();
    const pending = fetchWithTimeout(
      "https://example.test",
      {},
      5_000,
      hangingFetch,
    );
    const outcome = expect(pending).rejects.toMatchObject({
      name: "TimeoutError",
    });
    await vi.advanceTimersByTimeAsync(5_000);
    await outcome;
  });

  it("still aborts when the caller signal aborts", async () => {
    const controller = new AbortController();
    const pending = fetchWithTimeout(
      "https://example.test",
      { signal: controller.signal },
      60_000,
      hangingFetch,
    );
    const outcome = expect(pending).rejects.toMatchObject({
      name: "AbortError",
    });
    controller.abort();
    await outcome;
  });

  it("rejects immediately when the caller signal is already aborted", async () => {
    await expect(
      fetchWithTimeout(
        "https://example.test",
        { signal: AbortSignal.abort() },
        60_000,
        hangingFetch,
      ),
    ).rejects.toMatchObject({ name: "AbortError" });
  });
});
