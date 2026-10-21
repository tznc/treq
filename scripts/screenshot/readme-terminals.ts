import { expect } from "vitest";
import { screen, waitFor } from "../../test/test-utils";
import { setRepoSetting } from "../../src/lib/api";
import { startGpuiAgent } from "../../test/gpui-sidebar";

type UserEvent = {
  click: (element: Element) => Promise<void>;
};

const TUI_STYLE = [
  "position:absolute",
  "inset:28px 0 0 0",
  "z-index:20",
  "overflow:hidden",
  "padding:8px 10px",
  "background:#1e1e1e",
  "color:#d4d4d4",
  'font-family:"JetBrains Mono",ui-monospace,Menlo,monospace',
  "font-size:11px",
  "line-height:1.45",
  "white-space:pre",
  "pointer-events:none",
].join(";");

const CLAUDE_PEACH = "#e8a87c";
const CLAUDE_DIM = "#b8845c";

/** Matches the Claude Code startup TUI: peach mono, dashed frame, welcome + prompt. */
const CLAUDE_TUI = `<div data-testid="marketing-tui-claude" style="${TUI_STYLE};color:${CLAUDE_PEACH};line-height:1.2;font-size:10.5px;letter-spacing:0">
<span style="color:${CLAUDE_PEACH}">+-- Claude Code v2.0.0 -----------+</span>
<span style="color:${CLAUDE_PEACH}">|                                 |</span>
<span style="color:${CLAUDE_PEACH}">|         Welcome back            |</span>
<span style="color:${CLAUDE_PEACH}">|                                 |</span>
<span style="color:${CLAUDE_PEACH}">|             .^.                 |</span>
<span style="color:${CLAUDE_PEACH}">|            /# #\\                |</span>
<span style="color:${CLAUDE_PEACH}">|             '#'                 |</span>
<span style="color:${CLAUDE_PEACH}">|                                 |</span>
<span style="color:${CLAUDE_PEACH}">|      Sonnet 4.5 • Max 20x       |</span>
<span style="color:${CLAUDE_PEACH}">|      ~/event-bus                |</span>
<span style="color:${CLAUDE_PEACH}">+---------------------------------+</span>
<span style="color:${CLAUDE_DIM}">  Recent activity</span>
<span style="color:${CLAUDE_DIM}">  1m ago   Inspect Home.tsx</span>
<span style="color:${CLAUDE_DIM}">  8m ago   Handle empty events</span>
<span style="color:${CLAUDE_DIM}">  ... /resume for more</span>
<span style="color:${CLAUDE_PEACH}">-----------------------------------</span>
<span style="color:${CLAUDE_PEACH}">&gt; </span><span style="background:#f4f4f5;color:#1e1e1e"> </span><span style="color:${CLAUDE_DIM}">try "edit &lt;filepath&gt; to ..."</span>
</div>`;

const CODEX_TUI = `<div data-testid="marketing-tui-codex" style="${TUI_STYLE}">
<span style="color:#f4f4f5">codex</span>  <span style="color:#737373">gpt-5 · ~/event-bus</span>

<span style="color:#a1a1aa">• Exploring stacked PR #418</span>
<span style="color:#52525b">  └ packages/api/src/ingest.ts</span>
<span style="color:#52525b">  └ packages/web/src/lib/client.ts</span>

<span style="color:#60a5fa">› Add retry/backoff when event_message is empty and keep the
  Discord payload intact for the parent ingest PR.</span>

<span style="color:#4ade80">✔</span> <span style="color:#a1a1aa">Mapped empty-body fallback to EventBody</span>
<span style="color:#fbbf24">…</span> <span style="color:#a1a1aa">Waiting on CI: test failed, typecheck pending</span>

<span style="color:#27272a">────────────────────────────────</span>
<span style="color:#a1a1aa">  tokens  12.4k</span>   <span style="color:#737373">esc interrupt</span>
</div>`;

const CURSOR_TUI = `<div data-testid="marketing-tui-cursor" style="${TUI_STYLE}">
<span style="color:#f4f4f5">Cursor Agent</span>  <span style="color:#818cf8">composer</span>
<span style="color:#52525b">feat/empty-event-message → feat/event-ingest</span>

<span style="color:#a1a1aa"> I'll resolve the Home.tsx conflict in packages/web and keep
 the event feed from this stacked change.</span>

<span style="color:#818cf8">packages/web/src/pages/Home.tsx</span>
<span style="color:#f87171">- &lt;h1&gt;Lorem ipsum dolor sit amet&lt;/h1&gt;</span>
<span style="color:#4ade80">+ &lt;h1&gt;Lorem ipsum — event feed&lt;/h1&gt;</span>
<span style="color:#4ade80">+ &lt;EventFeed source="discord" /&gt;</span>

<span style="color:#a1a1aa"> CI on #418: 2/4 passing (test failed).</span>
<span style="color:#52525b"> Ask · Tab to accept · Esc to reject</span>
</div>`;

function tuiForHeader(text: string): string | null {
  if (/codex/i.test(text)) return CODEX_TUI;
  if (/cursor/i.test(text)) return CURSOR_TUI;
  if (/claude/i.test(text)) return CLAUDE_TUI;
  return null;
}

/**
 * Open Claude, Codex, and Cursor agent terminals with the workspace row's
 * "Start agent" sidebar button, which starts the repo's default agent.
 */
export async function openMarketingAgentTerminals(
  repoPath: string,
  branch: string,
): Promise<void> {
  const agents = ["claude", "codex", "cursor"] as const;
  for (const [index, agent] of agents.entries()) {
    await setRepoSetting(repoPath, "default_agent", agent);
    await startGpuiAgent(branch);
    await waitFor(() => {
      expect(
        document.querySelectorAll('[data-terminal-id^="agent-"]').length,
      ).toBe(index + 1);
    });
  }
}

export function expandMarketingTerminalPane(): void {
  const pane = document.querySelector(
    '[data-testid="workspace-terminal-pane"]',
  ) as HTMLElement | null;
  if (!pane) return;
  pane.style.height = "40%";
  pane.style.maxHeight = "60%";
}

export function hideMarketingTerminalPane(): void {
  const pane = document.querySelector(
    '[data-testid="workspace-terminal-pane"]',
  ) as HTMLElement | null;
  if (!pane) return;
  pane.style.display = "none";
}

export function showMarketingTerminalPane(): void {
  const pane = document.querySelector(
    '[data-testid="workspace-terminal-pane"]',
  ) as HTMLElement | null;
  if (!pane) return;
  pane.style.display = "";
}

/** Overlay TUI HTML — xterm paints to canvas, which does not serialize into the screenshot DOM. */
export function injectMarketingTuiOverlays(): void {
  for (const panel of document.querySelectorAll<HTMLElement>(
    "[data-terminal-id]",
  )) {
    if (panel.querySelector("[data-testid^='marketing-tui-']")) continue;
    panel.style.position = "relative";
    const headerText = panel.querySelector("span.truncate")?.textContent ?? "";
    const html = tuiForHeader(headerText);
    if (!html) continue;
    panel.insertAdjacentHTML("beforeend", html);
  }
}

export async function expandMarketingFileTree(user: UserEvent): Promise<void> {
  const clickNth = async (name: string, index = 0) => {
    const buttons = await screen.findAllByRole("button", { name });
    const target = buttons[index];
    if (!target) throw new Error(`No directory button named ${name} at ${index}`);
    await user.click(target);
  };

  await clickNth("packages");
  await clickNth("api");
  await clickNth("src", 0);
  await clickNth("web");
  await clickNth("src", 1);
  await clickNth("pages");
  await clickNth("lib");
}
