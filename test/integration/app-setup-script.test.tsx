// @include-serial
import * as React from "react";
import { beforeEach, describe, expect, it } from "vitest";
import { createTestRepo, openRepo } from "../utils";
import { getSetting, saveAppSetupScript } from "../../src/lib/api";
import { render, screen, waitFor } from "../test-utils";
import { Dashboard } from "../../src/components/Dashboard";
import userEvent from "@testing-library/user-event";

describe("application setup script", () => {
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
  }

  it("runs a saved script once and shows when it last ran", async () => {
    await openApplicationSettings();
    expect(await screen.findByText("Never run")).toBeTruthy();

    await user.type(
      screen.getByLabelText("Application Setup Script"),
      "echo setup",
    );
    await user.click(
      screen.getByRole("switch", { name: "Run on every app startup" }),
    );
    await user.click(screen.getByRole("button", { name: /save settings/i }));

    expect(await screen.findByText(/Last run .* passed/)).toBeTruthy();
    expect(await getSetting("app_setup_script")).toBe("echo setup");
    expect(await getSetting("app_setup_script_always_run")).toBe("true");
    expect(await getSetting("app_setup_script_last_hash")).toMatch(
      /^[0-9a-f]{64}$/,
    );
  });

  it("shows a failed run", async () => {
    await openApplicationSettings();
    await user.type(
      await screen.findByLabelText("Application Setup Script"),
      "exit 1",
    );
    await user.click(screen.getByRole("button", { name: /save settings/i }));

    await waitFor(() => expect(screen.getByText(/Last run .* failed/)));
  });
});
