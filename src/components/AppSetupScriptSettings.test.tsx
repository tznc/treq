import { describe, expect, it, vi } from "vitest";
import userEvent from "@testing-library/user-event";
import { render, screen } from "../../test/test-utils";
import type { AppSetupScriptStatus } from "../lib/api";
import { AppSetupScriptSettings } from "./AppSetupScriptSettings";

const SAVED: AppSetupScriptStatus = {
  script: "echo hi",
  always_run: false,
  last_run_at: null,
  last_status: null,
  running: false,
};

function renderSettings(
  props: Partial<React.ComponentProps<typeof AppSetupScriptSettings>> = {},
) {
  return render(
    <AppSetupScriptSettings
      script={props.status?.script ?? ""}
      alwaysRun={false}
      onScriptChange={vi.fn()}
      onAlwaysRunChange={vi.fn()}
      onRun={vi.fn().mockResolvedValue(undefined)}
      {...props}
    />,
  );
}

describe("AppSetupScriptSettings", () => {
  // An edit before the status loads would save defaults over the stored script.
  it("disables editing until the saved status loads", () => {
    renderSettings();

    expect(screen.getByLabelText("Application Setup Script")).toBeDisabled();
    expect(
      screen.getByRole("switch", { name: "Run on every app startup" }),
    ).toHaveAttribute("data-disabled");
    expect(screen.getByRole("button", { name: "Run" })).toBeDisabled();
  });

  it("shows why the status failed to load", () => {
    renderSettings({ loadError: "database is locked" });

    expect(
      screen.getByText("Could not load the setup script: database is locked"),
    ).toBeTruthy();
  });

  it("starts one run for a double click", async () => {
    const user = userEvent.setup();
    const onRun = vi.fn(() => new Promise<void>(() => {}));
    renderSettings({ status: SAVED, onRun });

    await user.dblClick(screen.getByRole("button", { name: "Run" }));

    expect(onRun).toHaveBeenCalledTimes(1);
    expect(screen.getByRole("button", { name: "Run" })).toBeDisabled();
  });
});
