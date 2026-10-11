import { openUrl } from "@tauri-apps/plugin-opener";
import userEvent from "@testing-library/user-event";
import { describe, expect, it, vi } from "vitest";
import { render, screen } from "../../../test/test-utils";
import type { InstanceStatusResponse } from "../../lib/api-types-remote";
import { CloudWorkspaceCard } from "./CloudWorkspaceCard";

function statusWith(status: string): InstanceStatusResponse {
  return {
    instance: {
      instance_id: "instance-1",
      status,
      provider_resource_id: "treq-user-1",
      generation: 0,
      disk_quota_gb: 5,
    },
    endpoint: null,
  } as never;
}

function renderCard(instanceStatus: InstanceStatusResponse) {
  const onProvision = vi.fn().mockResolvedValue(undefined);
  render(
    <CloudWorkspaceCard
      instanceStatus={instanceStatus}
      onProvision={onProvision}
      onWake={vi.fn()}
      onRepair={vi.fn()}
      onDelete={vi.fn()}
    />,
  );
  return onProvision;
}

describe("CloudWorkspaceCard", () => {
  it("offers to create a new cloud workspace after the old one was deleted", async () => {
    const user = userEvent.setup();
    const onProvision = renderCard(statusWith("deleted"));

    expect(
      screen.queryByRole("button", { name: "Delete cloud workspace" }),
    ).not.toBeInTheDocument();
    await user.click(
      screen.getByRole("button", { name: "Create cloud workspace" }),
    );
    expect(onProvision).toHaveBeenCalledOnce();
  });

  it("offers Upgrade to Pro instead of an error when the server requires Pro", async () => {
    const user = userEvent.setup();
    render(
      <CloudWorkspaceCard
        instanceStatus={statusWith("suspended")}
        provisioningError={
          "[pro_required] Cloud workspaces need Pro. You can still check the status of your cloud workspace or delete it.\nHTTP 402 · Correlation ID: c-1"
        }
        onProvision={vi.fn()}
        onWake={vi.fn()}
        onRepair={vi.fn()}
        onDelete={vi.fn()}
      />,
    );

    expect(screen.queryByRole("alert")).not.toBeInTheDocument();
    expect(screen.getByText(/Cloud workspaces need Pro\./)).toBeVisible();
    expect(screen.queryByText(/HTTP 402/)).not.toBeInTheDocument();
    await user.click(screen.getByRole("button", { name: "Upgrade to Pro" }));
    expect(openUrl).toHaveBeenCalledWith(expect.stringMatching(/\/dashboard$/));
    // Delete stays available to a user without Pro.
    expect(
      screen.getByRole("button", { name: "Delete cloud workspace" }),
    ).toBeInTheDocument();
  });

  it("still shows other failures as an error", () => {
    render(
      <CloudWorkspaceCard
        instanceStatus={statusWith("failed")}
        provisioningError={"[internal_error] Internal error\nHTTP 500"}
        onProvision={vi.fn()}
        onWake={vi.fn()}
        onRepair={vi.fn()}
        onDelete={vi.fn()}
      />,
    );
    expect(screen.getByRole("alert")).toHaveTextContent(
      "[internal_error] Internal error",
    );
    expect(
      screen.queryByRole("button", { name: "Upgrade to Pro" }),
    ).not.toBeInTheDocument();
  });

  it("manages a live cloud workspace instead of offering to create one", () => {
    renderCard(statusWith("suspended"));

    expect(
      screen.getByRole("button", { name: "Delete cloud workspace" }),
    ).toBeInTheDocument();
    expect(
      screen.queryByRole("button", { name: "Create cloud workspace" }),
    ).not.toBeInTheDocument();
  });
});
