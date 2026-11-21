import { claudeUrl, codexAppUrl, type LineAt, linePrompt, treqKickoffUrl } from "./links";

type Provider = { name: string; url: (at: LineAt, prompt: string) => string };

const PROVIDERS: Provider[] = [
  { name: "Claude", url: (at, prompt) => claudeUrl(at.repo, prompt) },
  { name: "Codex", url: (_, prompt) => codexAppUrl(prompt) },
  { name: "Treq", url: (at, prompt) => treqKickoffUrl(at.repo, at.ref, prompt) },
];

/** Opens a web link in a new tab and an app link (`treq://`, `codex://`) in place. */
export function openUrl(url: string): void {
  if (url.startsWith("https:")) window.open(url, "_blank", "noopener");
  else location.href = url;
}

/**
 * Per-line provider buttons beside a hovered code line, and the prompt
 * box one opens under it. Drawn over the page, so GitHub's re-renders
 * leave it alone.
 */
export class LinePrompt {
  readonly root: HTMLElement;
  private panelOpen = false;

  constructor(
    private doc: Document,
    private open: (url: string) => void = openUrl,
  ) {
    this.root = doc.createElement("div");
    this.root.className = "treq-line";
  }

  /** Shows the buttons for `at`, drawn at `box`; null hides them. Ignored while a prompt is open. */
  hover(at: LineAt | null, box: DOMRect | null): void {
    if (this.panelOpen) return;
    this.root.replaceChildren();
    if (!at || !box) return;
    const bar = this.place(this.doc.createElement("div"), box.top, box.right);
    bar.className = "treq-line__providers";
    for (const provider of PROVIDERS) {
      const button = this.doc.createElement("button");
      button.type = "button";
      button.textContent = provider.name;
      button.setAttribute("aria-label", `Prompt ${provider.name} about line ${at.line}`);
      button.addEventListener("click", () => this.prompt(provider, at, box));
      bar.append(button);
    }
    this.mount(bar);
  }

  private prompt(provider: Provider, at: LineAt, box: DOMRect): void {
    this.root.replaceChildren();
    this.panelOpen = true;
    const panel = this.place(this.doc.createElement("div"), box.bottom, box.right);
    panel.className = "treq-line__panel";
    const input = this.doc.createElement("textarea");
    input.placeholder = `Prompt ${provider.name} about ${at.path}:${at.line} (Enter to start, Esc to cancel)`;
    input.setAttribute("aria-label", `Prompt for ${provider.name}`);
    input.addEventListener("keydown", (event) => {
      if (event.key === "Escape") this.close();
      if (event.key !== "Enter" || event.shiftKey) return;
      event.preventDefault();
      const text = input.value.trim();
      if (!text) return;
      this.open(provider.url(at, linePrompt(at, text)));
      this.close();
    });
    panel.append(input);
    this.mount(panel);
    input.focus();
  }

  private close(): void {
    this.panelOpen = false;
    this.root.replaceChildren();
  }

  /** Right-aligns `el` at `right`, `top` px down the viewport, in page coordinates. */
  private place(el: HTMLElement, top: number, right: number): HTMLElement {
    const view = this.doc.defaultView;
    el.style.top = `${top + (view?.scrollY ?? 0)}px`;
    el.style.right = `${(this.doc.documentElement.clientWidth || right) - right - (view?.scrollX ?? 0)}px`;
    return el;
  }

  private mount(el: HTMLElement): void {
    this.root.append(el);
    if (!this.root.isConnected) this.doc.body.append(this.root);
  }
}
