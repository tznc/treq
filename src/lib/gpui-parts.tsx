import {
  createContext,
  type ReactNode,
  useCallback,
  useContext,
  useEffect,
  useRef,
  useState,
} from "react";

/**
 * Lets the components of one React screen hand their state to the component
 * that pushes that screen to GPUI. A screen like Settings keeps each tab's
 * drafts inside the tab's own component; each one publishes its part with
 * `useGpuiPart`, and the screen reads them all with `useGpuiPartsHost`.
 *
 * A part is plain data for the snapshot plus the handlers GPUI's actions
 * call, so the GPUI screen runs the same code the React one does.
 */

interface Part {
  data: unknown;
  handlers: Record<string, (...args: never[]) => unknown>;
}

interface Registry {
  set: (key: string, json: string, part: Part) => void;
  remove: (key: string) => void;
}

const PartsContext = createContext<Registry | null>(null);

/** The parts published below `GpuiPartsProvider`, by key. */
export function useGpuiPartsHost(): {
  parts: Record<string, unknown>;
  handlers: (key: string) => Part["handlers"] | undefined;
  registry: Registry;
} {
  const partsRef = useRef(new Map<string, { json: string; part: Part }>());
  const [parts, setParts] = useState<Record<string, unknown>>({});
  const publish = useCallback(() => {
    setParts(
      Object.fromEntries(
        [...partsRef.current].map(([key, { part }]) => [key, part.data]),
      ),
    );
  }, []);
  const registry = useRef<Registry>({
    set: (key, json, part) => {
      const previous = partsRef.current.get(key);
      partsRef.current.set(key, { json, part });
      if (previous?.json !== json) publish();
    },
    remove: (key) => {
      if (partsRef.current.delete(key)) publish();
    },
  }).current;
  const handlers = useCallback(
    (key: string) => partsRef.current.get(key)?.part.handlers,
    [],
  );
  return { parts, handlers, registry };
}

export function GpuiPartsProvider({
  registry,
  children,
}: {
  registry: Registry;
  children: ReactNode;
}) {
  return (
    <PartsContext.Provider value={registry}>{children}</PartsContext.Provider>
  );
}

/**
 * Publishes `data` (JSON-serializable) under `key` while mounted. `handlers`
 * may change every render; the latest ones are called.
 */
export function useGpuiPart<H extends Part["handlers"]>(
  key: string,
  data: unknown,
  handlers: H,
): void {
  const registry = useContext(PartsContext);
  const json = JSON.stringify(data);
  const part = useRef<Part>({ data, handlers });
  part.current.handlers = handlers;
  useEffect(() => {
    if (!registry) return;
    part.current.data = JSON.parse(json);
    registry.set(key, json, part.current);
  }, [registry, key, json]);
  useEffect(() => {
    if (!registry) return;
    return () => registry.remove(key);
  }, [registry, key]);
}
