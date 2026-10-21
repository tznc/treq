// @include-parallel
import * as React from "react";
import { beforeEach, describe, expect, it } from "vitest";
import userEvent from "@testing-library/user-event";
import { Dashboard } from "../../src/components/Dashboard";
import { getSetting, setSetting } from "../../src/lib/api";
import { render, screen } from "../test-utils";
import { createTestRepo, openRepo } from "../utils";

import { waitFor as waitForSnapshot } from "@testing-library/react";
import { findGpuiToast, gpuiSnapshot, sendGpuiAction } from "../gpui-sidebar";

async function openGpuiSettings() {
  await waitForSnapshot(() => expect(gpuiSnapshot()).toBeTruthy());
  sendGpuiAction({ kind: "openSettings" });
}

describe("Notify when an agent finishes setting", () => {
  let user: ReturnType<typeof userEvent.setup>;

  beforeEach(async () => {
    const { repoPath } = createTestRepo(false);
    openRepo(repoPath);
    await setSetting("notify_agent_finished", "");
    user = userEvent.setup();
  });

  const openApplicationSettings = async () => {
    await openGpuiSettings();
    await user.click(await screen.findByRole("tab", { name: /application/i }));
    return screen.findByRole("switch", {
      name: "Notify when an agent finishes",
    });
  };

  it("is on by default", async () => {
    render(<Dashboard />);
    const toggle = await openApplicationSettings();
    expect(toggle).toHaveAttribute("aria-checked", "true");
  });

  it("saves the switch turned off and shows it off when settings reopen", async () => {
    render(<Dashboard />);
    await user.click(await openApplicationSettings());
    await user.click(screen.getByRole("button", { name: /save settings/i }));
    await findGpuiToast("Settings Saved");

    expect(await getSetting("notify_agent_finished")).toBe("false");

    await user.click(screen.getByRole("button", { name: "Close" }));
    const reopened = await openApplicationSettings();
    expect(reopened).toHaveAttribute("aria-checked", "false");
  });
});
