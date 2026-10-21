import { afterEach, describe, expect, it, vi } from "vitest";
import {
  dismissTopWindowModal,
  setWindowOffsetLeft,
  windowModalBackdrop,
} from "./window-modals";

function mount(html: string) {
  document.body.innerHTML = html;
}

afterEach(() => {
  document.body.innerHTML = "";
  setWindowOffsetLeft(null);
});

describe("windowModalBackdrop", () => {
  it("is null with no modal open", () => {
    mount("<div role='dialog'>popover</div>");
    expect(windowModalBackdrop()).toBeNull();
  });

  it("reads the backdrop's alpha", () => {
    mount(
      "<div data-window-modal><div data-window-modal-backdrop style='background-color: rgba(0, 0, 0, 0.6)'></div></div>",
    );
    expect(windowModalBackdrop()).toBeCloseTo(0.6);
  });

  it("falls back to 40% when the colour has no readable alpha", () => {
    mount("<div cmdk-overlay></div><div cmdk-dialog></div>");
    expect(windowModalBackdrop()).toBe(0.4);
  });
});

describe("dismissTopWindowModal", () => {
  it("clicks the topmost backdrop only", () => {
    mount(
      "<div data-window-modal-backdrop id='under'></div><div cmdk-overlay id='top'></div>",
    );
    const under = vi.fn();
    const top = vi.fn();
    document.getElementById("under")!.addEventListener("click", under);
    document.getElementById("top")!.addEventListener("click", top);
    dismissTopWindowModal();
    expect(top).toHaveBeenCalledOnce();
    expect(under).not.toHaveBeenCalled();
  });
});

describe("setWindowOffsetLeft", () => {
  it("sets and clears the offset variable", () => {
    setWindowOffsetLeft(280);
    expect(
      document.documentElement.style.getPropertyValue("--window-offset-left"),
    ).toBe("280px");
    setWindowOffsetLeft(null);
    expect(
      document.documentElement.style.getPropertyValue("--window-offset-left"),
    ).toBe("");
  });
});
