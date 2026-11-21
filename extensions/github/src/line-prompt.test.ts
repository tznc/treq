import { beforeEach, describe, expect, it, vi } from "vitest";
import { LinePrompt } from "./line-prompt";

const at = { repo: "tznc/treq", ref: "main", path: "src/a.rs", line: 4 };
const box = { top: 100, bottom: 120, left: 10, right: 500 } as DOMRect;

describe("LinePrompt", () => {
  let open: ReturnType<typeof vi.fn>;
  let ui: LinePrompt;
  beforeEach(() => {
    document.body.innerHTML = "";
    open = vi.fn();
    ui = new LinePrompt(document, open);
  });

  const buttons = () => [...document.querySelectorAll<HTMLButtonElement>(".treq-line__providers button")];

  it("offers each provider on a hovered line", () => {
    ui.hover(at, box);
    expect(buttons().map((b) => b.getAttribute("aria-label"))).toEqual([
      "Prompt Claude about line 4",
      "Prompt Codex about line 4",
      "Prompt Treq about line 4",
    ]);
  });

  it("kicks off a typed prompt in the chosen provider", () => {
    ui.hover(at, box);
    buttons()[2].click();
    const input = document.querySelector<HTMLTextAreaElement>(".treq-line__panel textarea")!;
    input.value = "why?";
    input.dispatchEvent(new KeyboardEvent("keydown", { key: "Enter" }));
    expect(open).toHaveBeenCalledWith(
      "treq://agent/kickoff?repo=tznc%2Ftreq&branch=main&prompt=src%2Fa.rs%3A4%20(tznc%2Ftreq%40main)%0A%0Awhy%3F",
    );
    expect(document.querySelector(".treq-line__panel")).toBeNull();
  });

  it("does not start an empty prompt, and Escape closes it", () => {
    ui.hover(at, box);
    buttons()[0].click();
    const input = document.querySelector<HTMLTextAreaElement>(".treq-line__panel textarea")!;
    input.dispatchEvent(new KeyboardEvent("keydown", { key: "Enter" }));
    expect(open).not.toHaveBeenCalled();
    input.dispatchEvent(new KeyboardEvent("keydown", { key: "Escape" }));
    expect(document.querySelector(".treq-line__panel")).toBeNull();
  });

  it("hides the buttons when no line is hovered", () => {
    ui.hover(at, box);
    ui.hover(null, null);
    expect(buttons()).toHaveLength(0);
  });
});
