import { ICONS } from "./icons";
import { claudePromptUrl, codexAppUrl, treqKickoffUrl } from "./links";
import { type CodeAt, codePrompt } from "./prompt";

export type Provider = "claude" | "codex" | "treq";

const AGENTS: Record<
  Provider,
  { label: string; url: (at: CodeAt, prompt: string) => string }
> = {
  claude: {
    label: "Claude",
    url: (at, prompt) => claudePromptUrl(at.repo, prompt),
  },
  codex: { label: "Codex", url: (_, prompt) => codexAppUrl(prompt) },
  treq: {
    label: "Treq",
    url: (at, prompt) => treqKickoffUrl(at.repo, at.ref, prompt),
  },
};

/** Opens a web link in a new tab and an app link (`treq://`, `codex://`) in place. */
export function openUrl(url: string): void {
  if (url.startsWith("https:")) window.open(url, "_blank", "noopener");
  else location.href = url;
}

const SEND_HINT = /Mac|iPhone|iPad/.test(navigator.platform)
  ? "⌘ + Enter"
  : "Ctrl + Enter";

const where = (at: CodeAt) => {
  const [first, last] = [at.lines[0].line, at.lines[at.lines.length - 1].line];
  return `${at.path}:${first === last ? first : `${first}-${last}`}`;
};

/**
 * An icon button per agent beside a hovered code line, and the composer
 * one opens under it, laid out like the desktop app's. Drawn over the
 * page, so GitHub's re-renders leave it alone.
 */
export class LineComposer {
  readonly root: HTMLElement;
  private composing = false;

  constructor(
    private doc: Document,
    private open: (url: string) => void = openUrl,
    public providers: Provider[] = ["treq"],
  ) {
    this.root = doc.createElement("div");
    this.root.className = "treq-line";
  }

  /** Shows the buttons for `at`, drawn at `box`; null hides them. Ignored while composing. */
  hover(at: CodeAt | null, box: DOMRect | null): void {
    if (this.composing) return;
    this.root.replaceChildren();
    if (!at || !box) return;
    const bar = this.place(
      this.el("div", "treq-line__icons"),
      box.top,
      box.right,
    );
    for (const provider of this.providers) {
      const { label } = AGENTS[provider];
      const button = this.el("button", "treq-line__icon");
      button.type = "button";
      button.innerHTML = ICONS[provider];
      button.dataset.tip = `Prompt ${label}`;
      button.setAttribute("aria-label", `Prompt ${label} about ${where(at)}`);
      button.addEventListener("click", () => this.compose(provider, at, box));
      bar.append(button);
    }
    this.mount(bar);
  }

  private compose(provider: Provider, at: CodeAt, box: DOMRect): void {
    const { label, url } = AGENTS[provider];
    this.root.replaceChildren();
    this.composing = true;
    const panel = this.place(
      this.el("div", "treq-composer"),
      box.bottom,
      box.right,
    );
    const input = this.el("textarea");
    input.placeholder = `Ask ${label} about these lines`;
    input.setAttribute("aria-label", `Prompt for ${label}`);

    const context = this.el("span", "treq-composer__context");
    context.textContent = where(at);
    const agent = this.el("span", "treq-composer__agent");
    agent.innerHTML = ICONS[provider];
    agent.append(label);
    const send = this.el("button", "treq-composer__send");
    send.type = "button";
    send.disabled = true;
    send.innerHTML = ICONS.send;
    send.dataset.tip = SEND_HINT;
    send.setAttribute("aria-label", "Send");

    const submit = () => {
      const text = input.value.trim();
      if (!text) return;
      this.open(url(at, codePrompt(at, text)));
      this.close();
    };
    input.addEventListener("input", () => {
      send.disabled = !input.value.trim();
    });
    input.addEventListener("keydown", (event) => {
      if (event.key === "Escape") this.close();
      if (event.key === "Enter" && (event.metaKey || event.ctrlKey)) {
        event.preventDefault();
        submit();
      }
    });
    send.addEventListener("click", submit);

    const actions = this.el("div", "treq-composer__actions");
    actions.append(agent, send);
    const footer = this.el("div", "treq-composer__footer");
    footer.append(context, actions);
    panel.append(input, footer);
    this.mount(panel);
    input.focus();
  }

  private close(): void {
    this.composing = false;
    this.root.replaceChildren();
  }

  private el<K extends keyof HTMLElementTagNameMap>(
    tag: K,
    className = "",
  ): HTMLElementTagNameMap[K] {
    const el = this.doc.createElement(tag);
    el.className = className;
    return el;
  }

  /** Right-aligns `el` at `right`, `top` px down the viewport, in page coordinates. */
  private place<T extends HTMLElement>(el: T, top: number, right: number): T {
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
