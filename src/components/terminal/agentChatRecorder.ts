const AGENT_CHAT_CAPTURE_INTERVAL_MS = 1_000;

interface AgentChatRecorderOptions {
  register: () => Promise<unknown>;
  recordScreen: (screen: string) => Promise<unknown>;
  recordUserMessage: (screenBefore: string, text: string) => Promise<unknown>;
  getScreen: () => string;
  onError: (error: unknown) => void;
}

export interface AgentChatRecorder {
  output: () => void;
  idle: () => void;
  userMessage: (text: string) => Promise<void>;
  flush: () => Promise<void>;
  dispose: () => void;
}

/** Serializes chat persistence and periodically snapshots a busy xterm screen. */
export function createAgentChatRecorder({
  register,
  recordScreen,
  recordUserMessage,
  getScreen,
  onError,
}: AgentChatRecorderOptions): AgentChatRecorder {
  let disposed = false;
  let captureTimer: ReturnType<typeof setTimeout> | null = null;
  let writes = Promise.resolve()
    .then(register)
    .then(() => undefined)
    .catch(onError);

  const enqueue = (write: () => Promise<unknown>): Promise<void> => {
    writes = writes
      .then(write)
      .then(() => undefined)
      .catch(onError);
    return writes;
  };

  const capture = () => {
    if (disposed) return;
    const screen = getScreen();
    if (screen) void enqueue(() => recordScreen(screen));
  };

  return {
    output() {
      if (disposed || captureTimer != null) return;
      captureTimer = setTimeout(() => {
        captureTimer = null;
        capture();
      }, AGENT_CHAT_CAPTURE_INTERVAL_MS);
    },
    idle() {
      if (captureTimer != null) {
        clearTimeout(captureTimer);
        captureTimer = null;
      }
      capture();
    },
    userMessage(text) {
      const screenBefore = getScreen();
      return enqueue(() => recordUserMessage(screenBefore, text));
    },
    flush() {
      return writes;
    },
    dispose() {
      disposed = true;
      if (captureTimer != null) clearTimeout(captureTimer);
      captureTimer = null;
    },
  };
}
