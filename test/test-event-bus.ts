/**
 * Backs the mocked `@tauri-apps/api/event` `listen()` in tests.
 *
 * Unit tests have no event source, so listeners never fire (the old no-op
 * behavior). The NAPI-backed suites attach the backend's test event queue
 * with `setTestEventSource`; the bus polls it while anyone is listening and
 * delivers each event to the listeners registered for its name.
 */

type TestEvent = { event: string; payload: string };
type Handler = (event: { event: string; id: number; payload: unknown }) => void;

const POLL_MS = 16;

const listeners = new Map<string, Set<Handler>>();
let source: (() => TestEvent[]) | null = null;
let timer: ReturnType<typeof setInterval> | null = null;
let nextId = 1;

function deliver() {
  if (!source) return;
  for (const { event, payload } of source()) {
    for (const handler of listeners.get(event) ?? []) {
      handler({ event, id: nextId++, payload });
    }
  }
}

function syncPolling() {
  const active = source !== null && listeners.size > 0;
  if (active && timer === null) {
    timer = setInterval(deliver, POLL_MS);
  } else if (!active && timer !== null) {
    clearInterval(timer);
    timer = null;
  }
}

export function setTestEventSource(next: (() => TestEvent[]) | null) {
  source = next;
  syncPolling();
}

export function listenOnTestBus(
  event: string,
  handler: Handler,
): Promise<() => void> {
  let handlers = listeners.get(event);
  if (!handlers) {
    handlers = new Set();
    listeners.set(event, handlers);
  }
  handlers.add(handler);
  syncPolling();
  return Promise.resolve(() => {
    handlers.delete(handler);
    if (handlers.size === 0) listeners.delete(event);
    syncPolling();
  });
}

// PTYs spawned during a test (only when TREQ_TEST_PTY=1) are closed after it,
// so a fake agent never outlives the test that started it.
const openPtySessions = new Set<string>();

export function trackPtyInvoke(cmd: string, args?: Record<string, unknown>) {
  const sessionId = args?.sessionId;
  if (typeof sessionId !== "string") return;
  if (cmd === "pty_create_session") openPtySessions.add(sessionId);
  if (cmd === "pty_close") openPtySessions.delete(sessionId);
}

export async function closeTestPtys(
  invoke: (cmd: string, args?: Record<string, unknown>) => Promise<unknown>,
) {
  const sessions = [...openPtySessions];
  openPtySessions.clear();
  await Promise.all(
    sessions.map((sessionId) =>
      invoke("pty_close", { sessionId }).catch(() => {}),
    ),
  );
  // Drop output a closed PTY emitted after its listener went away.
  source?.();
}

/** Deliver an event to the listeners registered on the bus, as the backend would. */
export function emitOnTestBus(event: string, payload: unknown) {
  for (const handler of listeners.get(event) ?? []) {
    handler({ event, id: nextId++, payload });
  }
}
