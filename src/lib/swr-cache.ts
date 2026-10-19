import { type ReactNode, useEffect, useLayoutEffect, useRef } from "react";
import { mutate as defaultMutate, type Key, useSWRConfig } from "swr";

type ScopedMutate = typeof defaultMutate;

let scopedMutate: ScopedMutate = defaultMutate;

export function SWRMutateScope({ children }: { children: ReactNode }) {
  const { mutate } = useSWRConfig();
  useLayoutEffect(() => {
    scopedMutate = mutate as ScopedMutate;
    return () => {
      scopedMutate = defaultMutate;
    };
  }, [mutate]);
  return children;
}

export function keyMatchesPrefix(
  key: Key,
  prefix: readonly unknown[],
): boolean {
  if (!Array.isArray(key)) return false;
  if (key.length < prefix.length) return false;
  return prefix.every((part, i) => Object.is(key[i], part));
}

type InfiniteQuery = { key: readonly unknown[]; revalidate: () => unknown };

const infiniteQueries = new Set<InfiniteQuery>();

/**
 * SWR's filter form of `mutate` skips `useSWRInfinite` keys, so
 * `invalidateQueries` cannot reach them on its own. An infinite list calls
 * this with its key parts and its own `mutate` to be revalidated too.
 */
export function useInfiniteQueryInvalidation(
  key: readonly unknown[],
  revalidate: () => unknown,
) {
  const revalidateRef = useRef(revalidate);
  revalidateRef.current = revalidate;
  const serializedKey = JSON.stringify(key);
  useEffect(() => {
    const entry: InfiniteQuery = {
      key: JSON.parse(serializedKey) as unknown[],
      revalidate: () => revalidateRef.current(),
    };
    infiniteQueries.add(entry);
    return () => {
      infiniteQueries.delete(entry);
    };
  }, [serializedKey]);
}

/** Revalidate SWR keys whose array form starts with `prefix`. Omit prefix to revalidate all. */
export function invalidateQueries(prefix?: readonly unknown[]) {
  const infinite = [...infiniteQueries]
    .filter((q) => !prefix || keyMatchesPrefix(q.key as unknown[], prefix))
    .map((q) => q.revalidate());
  const regular = prefix
    ? scopedMutate((key) => keyMatchesPrefix(key, prefix))
    : scopedMutate(() => true);
  return Promise.all([regular, ...infinite]);
}

/** Revalidate the SWR keys that are one of `keys`, which `invalidateQueries` cannot match. */
export function revalidateKeys(keys: readonly string[]) {
  return scopedMutate((key) => typeof key === "string" && keys.includes(key));
}

export function setQueryData<T>(key: unknown[], data: T) {
  return scopedMutate(key, data, { revalidate: false });
}

export async function fetchAndCache<T>(
  key: unknown[],
  fn: () => Promise<T>,
): Promise<T> {
  const data = await fn();
  await scopedMutate(key, data, { revalidate: false });
  return data;
}

export async function clearSWRCache(): Promise<void> {
  await scopedMutate(() => true, undefined, { revalidate: false });
}

export const defaultSWRConfig = {
  revalidateOnFocus: false,
  errorRetryCount: 1,
} as const;

/** Background SWR polls fight jj's working-copy lock in integration tests. */
export function pollMs(ms: number): number {
  return import.meta.env.MODE === "test" ? 0 : ms;
}

export function createTestSWRConfig() {
  return {
    revalidateOnFocus: false as const,
    errorRetryCount: 0,
    provider: () => new Map(),
  };
}

export const testSWRConfig = createTestSWRConfig();
