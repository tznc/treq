import { describe, expect, it, vi } from "vitest";
import { toGpuiToasts } from "./toastStore";

describe("toGpuiToasts", () => {
  it("sends the GPUI shell each action's label, not its handler", () => {
    expect(
      toGpuiToasts([
        { id: "a", title: "Saved", type: "success" },
        {
          id: "b",
          title: "Push failed",
          description: "Rejected",
          type: "error",
          action: { label: "Retry", onClick: vi.fn() },
        },
      ]),
    ).toEqual([
      { id: "a", title: "Saved", type: "success" },
      {
        id: "b",
        title: "Push failed",
        description: "Rejected",
        type: "error",
        action: "Retry",
      },
    ]);
  });
});
