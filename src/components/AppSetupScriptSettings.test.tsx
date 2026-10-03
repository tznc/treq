import { describe, expect, it, vi } from "vitest";
import { render, screen } from "../../test/test-utils";
import { AppSetupScriptSettings } from "./AppSetupScriptSettings";

describe("AppSetupScriptSettings", () => {
  // An edit before the status loads would save defaults over the stored script.
  it("disables editing until the saved status loads", () => {
    render(
      <AppSetupScriptSettings
        script=""
        alwaysRun={false}
        onScriptChange={vi.fn()}
        onAlwaysRunChange={vi.fn()}
      />,
    );

    expect(screen.getByLabelText("Application Setup Script")).toBeDisabled();
    expect(
      screen.getByRole("switch", { name: "Run on every app startup" }),
    ).toHaveAttribute("data-disabled");
  });
});
