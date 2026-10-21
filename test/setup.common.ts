/**
 * Shared setup logic between unit tests (setup.ts) and integration tests
 * (setup.integration.ts). Import this file from each setup file.
 *
 * Includes: DOM polyfills, browser API mocks, Tauri plugin stubs, and hook mocks
 * that are identical across both test suites.
 *
 * NOT included here: @tauri-apps/api/core mock (differs: vi.fn() vs napi dispatch).
 */

import { afterEach, vi } from "vitest";
import { cleanup } from "@testing-library/react";
import { act } from "react";
import { configure, prettyDOM } from "@testing-library/dom";
import "@testing-library/jest-dom/vitest";

// ── Cleanup ───────────────────────────────────────────────────────────────────

const getReducedDomSnapshot = (container: HTMLElement): string => {
  const containerClone = container.cloneNode(true) as HTMLElement;

  for (const element of containerClone.querySelectorAll("*")) {
    element.removeAttribute("class");
    element.removeAttribute("style");
    element.removeAttribute("tabindex");
    element.removeAttribute("data-state");
  }

  for (const svg of containerClone.querySelectorAll("svg")) {
    svg.remove();
  }

  const divs = Array.from(containerClone.querySelectorAll("div")).reverse();
  for (const div of divs) {
    const ownText = Array.from(div.childNodes)
      .filter((node) => node.nodeType === Node.TEXT_NODE)
      .map((node) => node.textContent ?? "")
      .join("")
      .trim();
    const hasContent = ownText.length > 0;
    const isEmptyDiv =
      div.childElementCount === 0 && div.attributes.length === 0 && !hasContent;
    if (isEmptyDiv) {
      div.remove();
      continue;
    }

    const shouldUnwrap =
      div.attributes.length === 0 && div.childElementCount === 1 && !hasContent;
    if (shouldUnwrap) {
      const onlyChild = div.firstElementChild;
      if (onlyChild) {
        div.replaceWith(onlyChild);
      }
    }
  }

  return prettyDOM(containerClone) || "<empty />";
};

configure({
  asyncUtilTimeout: 5000,
  getElementError: (message, container) => {
    const reducedSnapshot = getReducedDomSnapshot(container as HTMLElement);
    return new Error(`${message}\n\n${reducedSnapshot}`);
  },
});

afterEach(async () => {
  await act(async () => {
    await Promise.resolve();
    await new Promise<void>((resolve) => setTimeout(resolve, 0));
  });
  cleanup();
  const { resetAllStores } = await import("../src/stores/resetStores");
  resetAllStores();
});

// ── DOM polyfills / browser API stubs ────────────────────────────────────────

(global as any).ResizeObserver = class ResizeObserver {
  constructor(cb: any) {
    (this as any).cb = cb;
  }

  observe(target?: Element) {
    (this as any).cb(
      [
        {
          target,
          borderBoxSize: [{ inlineSize: 800, blockSize: 600 }],
          contentBoxSize: [{ inlineSize: 800, blockSize: 600 }],
          contentRect: {
            x: 0,
            y: 0,
            width: 800,
            height: 600,
            top: 0,
            left: 0,
            bottom: 600,
            right: 800,
          },
        },
      ],
      this,
    );
  }

  unobserve() {}
  disconnect() {}
};

(global as any).IntersectionObserver = class IntersectionObserver {
  constructor(_cb: any) {}
  observe() {}
  unobserve() {}
  disconnect() {}
  takeRecords() {
    return [];
  }
};

(global as any).DOMRect = {
  fromRect: () => ({
    top: 0,
    left: 0,
    bottom: 0,
    right: 0,
    width: 0,
    height: 0,
  }),
};

// jsdom doesn't implement Range.getBoundingClientRect (real browsers do) --
// stub it so selection-driven UI (e.g. floating toolbars) can be exercised.
if (!Range.prototype.getBoundingClientRect) {
  Range.prototype.getBoundingClientRect = function () {
    return {
      x: 0,
      y: 0,
      top: 0,
      left: 0,
      right: 0,
      bottom: 0,
      width: 0,
      height: 0,
      toJSON: () => ({}),
    } as DOMRect;
  };
}
// Stub HTMLCanvasElement.getContext to silence jsdom "not implemented" warnings
HTMLCanvasElement.prototype.getContext = vi.fn(
  () => null,
) as typeof HTMLCanvasElement.prototype.getContext;

Element.prototype.scrollIntoView = vi.fn();

global.requestIdleCallback = vi.fn((callback) => {
  setTimeout(callback, 0);
  return 0;
}) as unknown as typeof requestIdleCallback;

global.cancelIdleCallback = vi.fn();

if (!("requestAnimationFrame" in globalThis)) {
  global.requestAnimationFrame = ((callback: FrameRequestCallback) =>
    setTimeout(() => callback(Date.now()), 0)) as typeof requestAnimationFrame;
}

if (!("cancelAnimationFrame" in globalThis)) {
  global.cancelAnimationFrame = ((handle: number) =>
    clearTimeout(handle)) as typeof cancelAnimationFrame;
}

Object.defineProperty(window, "matchMedia", {
  writable: true,
  value: vi.fn().mockImplementation((query: string) => ({
    matches: false,
    media: query,
    onchange: null,
    addListener: vi.fn(),
    removeListener: vi.fn(),
    addEventListener: vi.fn(),
    removeEventListener: vi.fn(),
    dispatchEvent: vi.fn(),
  })),
});

if (!navigator.clipboard) {
  Object.defineProperty(navigator, "clipboard", {
    value: { writeText: async (_text: string) => {} },
    writable: true,
    configurable: true,
  });
}
vi.spyOn(navigator.clipboard, "writeText").mockResolvedValue(undefined);

// ── Tauri API / plugin stubs (identical in both suites) ───────────────────────

vi.mock("@tauri-apps/api/event", async () => {
  const { listenOnTestBus } = await import("./test-event-bus");
  return {
    listen: vi.fn(listenOnTestBus),
    emit: vi.fn(),
  };
});

vi.mock("@tauri-apps/api/window", () => ({
  getCurrentWindow: vi.fn(() => ({
    label: "main",
    setTitle: vi.fn(),
    onFocusChanged: vi.fn(() => Promise.resolve(() => {})),
  })),
  WebviewWindow: vi.fn(),
}));

vi.mock("@tauri-apps/api/webviewWindow", async () => {
  const { listenOnTestBus } = await import("./test-event-bus");
  return {
    WebviewWindow: vi.fn(),
    getCurrentWebviewWindow: vi.fn(() => ({
      label: "main",
      listen: vi.fn(listenOnTestBus),
    })),
  };
});

vi.mock("@tauri-apps/plugin-dialog", () => ({
  selectFolder: vi.fn(),
  open: vi.fn(),
  ask: vi.fn(),
}));

vi.mock("@tauri-apps/plugin-opener", () => ({
  openPath: vi.fn(),
  revealItemInDir: vi.fn(),
  openUrl: vi.fn(),
}));

// Mock Radix/cmdk/Popover to avoid Portal/floating-ui issues in test env
vi.mock(
  "../src/components/ui/popover",
  async () => await import("./mocks/popover"),
);
vi.mock("cmdk", async () => await import("./mocks/cmdk"));

vi.mock("react-virtuoso", async () => {
  const React = await import("react");
  const Virtuoso = React.forwardRef(
    (
      {
        data,
        itemContent,
        scrollerRef,
        computeItemKey,
      }: {
        data?: unknown[];
        itemContent?: (index: number, item: unknown) => React.ReactNode;
        scrollerRef?: (el: HTMLElement | Window | null) => void;
        computeItemKey?: (index: number, item: unknown) => React.Key;
      },
      ref: React.Ref<{ scrollToIndex: (opts: unknown) => void }>,
    ) => {
      React.useImperativeHandle(ref, () => ({
        scrollToIndex: () => undefined,
      }));
      React.useEffect(() => {
        if (typeof scrollerRef === "function") {
          scrollerRef(document.createElement("div"));
        }
      }, [scrollerRef]);
      return React.createElement(
        "div",
        { "data-testid": "diff-virtuoso", className: "h-full overflow-auto" },
        (data ?? []).map((item, index) =>
          React.createElement(
            "div",
            { key: computeItemKey ? computeItemKey(index, item) : index },
            itemContent?.(index, item),
          ),
        ),
      );
    },
  );
  Virtuoso.displayName = "VirtuosoMock";
  return { Virtuoso };
});

// ── Hook mocks ────────────────────────────────────────────────────────────────

vi.mock("../src/hooks/useWorkspaceGitStatus", () => ({
  useWorkspaceGitStatus: vi.fn(() => ({
    status: null,
    branchInfo: null,
    divergence: null,
    lineDiffStats: null,
  })),
}));

vi.mock("../src/hooks/useCachedWorkspaceChanges", () => ({
  useCachedWorkspaceChanges: vi.fn(() => ({
    changes: [],
    isLoading: false,
    error: null,
    refresh: vi.fn(),
  })),
}));
