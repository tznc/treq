import { beforeEach, describe, expect, it, vi } from "vitest";
import { LineComposer } from "./composer";

const at = {
  repo: "tznc/treq",
  ref: "main",
  path: "src/a.rs",
  lines: [
    { line: 10, text: "a" },
    { line: 11, text: "b" },
  ],
};
const box = { top: 100, bottom: 120, left: 10, right: 500 } as DOMRect;

describe("LineComposer", () => {
  let open: ReturnType<typeof vi.fn>;
  beforeEach(() => {
    document.body.innerHTML = "";
    open = vi.fn();
  });

  const icons = () => [
    ...document.querySelectorAll<HTMLButtonElement>(".treq-line__icons button"),
  ];
  const input = () =>
    document.querySelector<HTMLTextAreaElement>(".treq-composer textarea")!;
  const send = () =>
    document.querySelector<HTMLButtonElement>(".treq-composer__send")!;
  const type = (text: string) => {
    input().value = text;
    input().dispatchEvent(new Event("input"));
  };
  const key = (init: KeyboardEventInit) =>
    input().dispatchEvent(new KeyboardEvent("keydown", init));

  it("shows an icon button with a tooltip per provider on a hovered line", () => {
    new LineComposer(document, open, ["claude", "treq"]).hover(at, box);
    expect(
      icons().map((b) => [
        b.getAttribute("aria-label"),
        b.dataset.tip,
        !!b.querySelector("svg"),
      ]),
    ).toEqual([
      ["Prompt Claude about src/a.rs:10-11", "Prompt Claude", true],
      ["Prompt Treq about src/a.rs:10-11", "Prompt Treq", true],
    ]);
  });

  it("shows only Treq by default", () => {
    new LineComposer(document, open).hover(at, box);
    expect(icons().map((b) => b.dataset.tip)).toEqual(["Prompt Treq"]);
  });

  it("opens a composer naming the lines, agent and a send button", () => {
    new LineComposer(document, open).hover(at, box);
    icons()[0].click();
    expect(document.querySelector(".treq-line__icons")).toBeNull();
    expect(input().getAttribute("aria-label")).toBe("Prompt for Treq");
    expect(document.querySelector(".treq-composer__context")?.textContent).toBe(
      "src/a.rs:10-11",
    );
    expect(document.querySelector(".treq-composer__agent")?.textContent).toBe(
      "Treq",
    );
    expect(send().getAttribute("aria-label")).toBe("Send");
    expect(send().disabled).toBe(true);
    type("why?");
    expect(send().disabled).toBe(false);
  });

  it("sends the prompt with its code on Cmd or Ctrl + Enter", () => {
    const ui = new LineComposer(document, open, ["codex", "treq"]);
    ui.hover(at, box);
    icons()[1].click();
    type("why?");
    key({ key: "Enter" });
    expect(open).not.toHaveBeenCalled();
    key({ key: "Enter", metaKey: true });
    expect(open).toHaveBeenCalledWith(
      `treq://agent/kickoff?repo=tznc%2Ftreq&branch=main&prompt=${encodeURIComponent("src/a.rs:10-11 (tznc/treq@main)\n```\n10  a\n11  b\n```\n\nwhy?")}`,
    );
    expect(document.querySelector(".treq-composer")).toBeNull();
    ui.hover(at, box);
    icons()[0].click();
    type("go");
    key({ key: "Enter", ctrlKey: true });
    expect(open).toHaveBeenLastCalledWith(
      expect.stringMatching(/^codex:\/\/new\?prompt=/),
    );
  });

  it("sends with the send button, never an empty prompt", () => {
    new LineComposer(document, open).hover(at, box);
    icons()[0].click();
    type("   ");
    key({ key: "Enter", metaKey: true });
    expect(open).not.toHaveBeenCalled();
    type("go");
    send().click();
    expect(open).toHaveBeenCalledTimes(1);
  });

  it("closes on Escape and keeps a composer open while hovering other lines", () => {
    const ui = new LineComposer(document, open);
    ui.hover(at, box);
    icons()[0].click();
    ui.hover(null, null);
    expect(input()).not.toBeNull();
    key({ key: "Escape" });
    expect(document.querySelector(".treq-composer")).toBeNull();
    ui.hover(at, box);
    ui.hover(null, null);
    expect(icons()).toHaveLength(0);
  });
});
