// @include-serial
import * as React from "react";
import { beforeEach, describe, expect, it } from "vitest";
import { createTestRepo, openRepo } from "../utils";
import { getSetting, saveAppSetupScript } from "../../src/lib/api";
import { render, screen, waitFor, within } from "../test-utils";
import { Dashboard } from "../../src/components/Dashboard";
import userEvent from "@testing-library/user-event";

describe("application setup script", { timeout: 20_000 }, () => {
  let user: ReturnType<typeof userEvent.setup>;

  beforeEach(async () => {
    const { repoPath } = createTestRepo(false);
    openRepo(repoPath);
    user = userEvent.setup();
    await saveAppSetupScript("", false);
  });

  async function openApplicationSettings() {
    render(<Dashboard />);
    await user.click(await screen.findByLabelText("Settings"));
    await user.click(await screen.findByRole("tab", { name: /application/i }));
    const script = await screen.findByLabelText("Application Setup Script");
    await waitFor(() => expect(script).toBeEnabled());
  }

  async function saveScript(script: string) {
    await user.type(screen.getByLabelText("Application Setup Script"), script);
    await user.click(screen.getByRole("button", { name: /save settings/i }));
    await waitFor(() =>
      expect(screen.getByRole("button", { name: /^run$/i })).toBeEnabled(),
    );
  }

  it("runs the saved script only when Run is clicked", async () => {
    await openApplicationSettings();
    expect(await screen.findByText("Never run")).toBeTruthy();
    const run = screen.getByRole("button", { name: /^run$/i });
    expect(run).toBeDisabled();

    await user.type(
      screen.getByLabelText("Application Setup Script"),
      "echo setup",
    );
    expect(run).toBeDisabled();
    expect(
      screen.getByText("Save settings to run the edited script."),
    ).toBeTruthy();

    await user.click(
      screen.getByRole("switch", { name: "Run on every app startup" }),
    );
    await user.click(screen.getByRole("button", { name: /save settings/i }));
    await waitFor(() => expect(run).toBeEnabled());
    expect(await getSetting("app_setup_script")).toBe("echo setup");
    expect(await getSetting("app_setup_script_always_run")).toBe("true");
    expect(screen.getByText("Never run")).toBeTruthy();

    await user.click(run);
    expect(await screen.findByText(/Last run .* passed/)).toBeTruthy();
  });

  it("shows a failed run", async () => {
    await openApplicationSettings();
    await saveScript("exit 1");
    await user.click(screen.getByRole("button", { name: /^run$/i }));

    await waitFor(() => expect(screen.getByText(/Last run .* failed/)));
  });

  it("shows run output in the Logs tab", async () => {
    const marker = `app-setup-${Date.now()}`;
    await openApplicationSettings();
    await saveScript(`echo ${marker}`);
    await user.click(screen.getByRole("button", { name: /^run$/i }));
    await screen.findByText(/Last run .* passed/);

    await user.click(screen.getByRole("button", { name: "Close" }));
    await user.click(await screen.findByRole("tab", { name: /^Logs/ }));
    await user.selectOptions(
      await screen.findByLabelText("Log source"),
      "app-setup",
    );

    const output = await screen.findByTestId("app-setup-logs-output");
    await waitFor(() => expect(within(output).getByText(marker)).toBeTruthy());
  });
});
