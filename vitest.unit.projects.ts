import react from "@vitejs/plugin-react";
import { VITEST_PROJECT_SEQUENCE } from "./vitest.projects";

const plugins = [
  react({
    babel: {
      plugins: [["babel-plugin-react-compiler", { target: "19" }]],
    },
  }),
];

/**
 * Hook, component and lib tests that need `window`/`document`/`localStorage`.
 * Every other `src/**\/*.test.ts` runs in the lean node project; a new test
 * that reaches for the DOM fails there with "document is not defined" and
 * belongs in this list.
 */
export const domTestFiles = [
  "src/components/changes-diff-viewer/hooks/useFileStaging.test.ts",
  "src/components/changes-diff-viewer/hooks/useLineSelection.test.ts",
  "src/components/changes-diff-viewer/hooks/useReview.viewed.test.ts",
  "src/components/changes-diff-viewer/useDiffVirtuosoItems.test.ts",
  "src/components/terminal/useAgentAutoCommand.test.ts",
  "src/components/workspace-terminal-pane/scrollTerminalFullyIntoView.test.ts",
  "src/components/workspace-terminal-pane/useTerminalSessionActions.test.ts",
  "src/components/workspace-terminal-pane/useTerminalSessionSummaries.test.ts",
  "src/hooks/useDebounce.test.ts",
  "src/hooks/useLinearAutoKickoff.test.ts",
  "src/hooks/useMergeQueueStatus.test.ts",
  "src/lib/agentDeepLink.test.ts",
  "src/lib/change-file-drag.test.ts",
  "src/lib/focus-refresh.test.ts",
  "src/lib/mobile-session.test.ts",
  "src/lib/repo-url.test.ts",
  "src/lib/utils.copyText.test.ts",
  "src/stores/authStore.freshness.test.ts",
  "src/stores/signOutCutoff.test.ts",
];

const shared = {
  plugins,
  test: {
    sequence: { groupOrder: VITEST_PROJECT_SEQUENCE.unit },
    globals: true,
    fileParallelism: true,
    testTimeout: 15000,
    env: { DEBUG_PRINT_LIMIT: "10" },
  },
};

/**
 * Fast unit tests: mocked Tauri invoke, no NAPI/jj required.
 * Safe to run with fileParallelism enabled.
 */
export const unitProjects = [
  {
    ...shared,
    test: {
      ...shared.test,
      name: "unit-node",
      environment: "node",
      setupFiles: ["./test/setup.node.ts"],
      include: ["src/**/*.test.ts"],
      exclude: domTestFiles,
    },
  },
  {
    ...shared,
    test: {
      ...shared.test,
      name: "unit",
      environment: "jsdom",
      setupFiles: ["./test/setup.ts"],
      include: [
        "src/**/*.test.tsx",
        ...domTestFiles,
        "test/merge-queue/**/*.test.ts",
        "test/linear-proxy/**/*.test.ts",
        "test/billing/**/*.test.ts",
        "test/vitest-config.test.ts",
        "test/remote-ssh-traceability.test.ts",
      ],
    },
  },
];
