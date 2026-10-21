# Frontend Test Guide

| Layer | Tool | Location |
|---|---|---|
| Unit | Vitest + React Testing Library | `test/*.test.ts(x)` |
| Integration | Vitest + RTL + real Rust via NAPI | `test/integration/**/*.test.tsx` |

Setup lives in `setup.common.ts` (DOM polyfills, Tauri API stubs) and `setup.integration.ts` (NAPI dispatch). Factories are in `factories/`, UI library mocks in `mocks/`, render helpers in `test-utils.tsx`.

## Integration test pattern

1. Create a real jj repository via NAPI (`createTestRepo`)
2. Set the active repo URL param (`openRepo`)
3. Render `<Dashboard />` — Tauri `invoke` is replaced with NAPI dispatch
4. Drive the UI via `userEvent`
5. Assert DOM state

```typescript
describe("workspace header", () => {
  let user: ReturnType<typeof userEvent.setup>;

  beforeEach(() => {
    const { repoPath } = createTestRepo(false);
    openRepo(repoPath);
    user = userEvent.setup();
  });

  it("switches branch on click", async () => {
    await createWorkspace(repoPath, "feat/thing");
    render(<Dashboard />);
    await clickGpuiRow("feat/thing");
    expect(await screen.findByText("feat/thing")).toBeTruthy();
  });
});
```

## Checklist

- Start every test from `createTestRepo()`; never share repo state between tests
- Render `<Dashboard />`, not individual components, so the full provider tree runs
- The workspace sidebar is native (GPUI). Drive it with `test/gpui-sidebar.ts`, which reads the snapshot React pushes and emits sidebar actions
- Use `userEvent`, never `fireEvent`
- Prefer `screen.findBy*` over `getBy*` for elements that appear after Rust calls resolve
- Assert visible text or ARIA roles, not internal state or which private functions ran
- Keep `beforeEach` minimal; move complex repo setup into the test or a shared helper
- Never mock the Rust backend in integration tests; use the real NAPI dispatch
- Start every integration file with `// @include-parallel` or `// @include-serial` on its own line; the Vitest config reads it to pick the project, and a file without one runs serially. Use serial for files that edit the working copy and drive the Changes or Review views

## Fake agent terminals

Tests never run a real agent CLI. To start agent sessions with a working terminal, call `installFakeAgents()` from `test/fake-agent.ts` and restore it afterwards (`try/finally` or `onTestFinished(installFakeAgents())`).

- `test/fake-agent/fake-agent.sh` stands in for `claude`, `codex`, `cursor-agent` and `copilot`. It prints `fake-agent: <cli>`, `mode:`, `model:`, `session:`, `ready` and `prompt:` lines, then echoes each typed line as `received: <line>`.
- The helper sets `TREQ_TEST_PTY=1`, so the backend spawns a real PTY without a Tauri `AppHandle`. PTY output reaches the mocked `listen()` through `test/test-event-bus.ts`, and xterm renders it, so `within(pane).findByText("mode: plan")` works.
- Without the helper, PTYs do not spawn in tests (the previous behavior).

## Unit environments

`src/**/*.test.ts` runs in the lean `unit-node` project (`setup.node.ts`). A test that needs `window`/`document` goes in `domTestFiles` in `vitest.unit.projects.ts` to run under jsdom with the full `setup.ts`; `.test.tsx` always runs under jsdom.
