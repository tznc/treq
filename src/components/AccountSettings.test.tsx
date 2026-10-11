import { render, screen, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { beforeEach, describe, expect, it, vi } from "vitest";
import {
  AccountSettings,
  type CloudWorkspaceControls,
} from "./AccountSettings";
import type {
  InstanceStatusResponse,
  ManagedInstanceState,
} from "../lib/api-types-remote";
import { defaultAuthState, useAuthStore } from "../stores/authStore";

vi.mock("../lib/api", () => ({
  getSetting: vi.fn(),
  setSetting: vi.fn(),
}));

vi.mock("@tauri-apps/plugin-opener", () => ({ openUrl: vi.fn() }));

function statusWith(status: ManagedInstanceState): InstanceStatusResponse {
  return {
    instance: {
      instance_id: "inst-1",
      provider_resource_id: "res-1",
      status,
      generation: 1,
      disk_quota_gb: 5,
    },
    endpoint: null,
  } as never;
}

function controls(
  overrides: Partial<CloudWorkspaceControls> = {},
): CloudWorkspaceControls {
  return {
    instanceStatus: null,
    onRefreshStatus: vi.fn().mockResolvedValue(undefined),
    onProvision: vi.fn().mockResolvedValue(undefined),
    onWake: vi.fn().mockResolvedValue(undefined),
    onRepair: vi.fn().mockResolvedValue(undefined),
    onDelete: vi.fn().mockResolvedValue(undefined),
    ...overrides,
  };
}

describe("AccountSettings", () => {
  beforeEach(() => {
    useAuthStore.setState({
      ...defaultAuthState,
      loading: false,
      user: {
        id: "user-1",
        email: "dev@example.com",
        user_metadata: {},
      } as never,
    });
  });

  it("creates a cloud workspace directly from the account page", async () => {
    const cloudWorkspace = controls();
    render(<AccountSettings cloudWorkspace={cloudWorkspace} />);

    expect(cloudWorkspace.onRefreshStatus).toHaveBeenCalledOnce();
    await userEvent.click(
      screen.getByRole("button", { name: "Create cloud workspace" }),
    );

    expect(cloudWorkspace.onProvision).toHaveBeenCalledOnce();
    expect(screen.queryByText(/Sprite/)).not.toBeInTheDocument();
  });

  it("shows the provisioning stage while creation is in flight", () => {
    render(
      <AccountSettings
        cloudWorkspace={controls({
          provisioningStage: "Requesting provisioning...",
        })}
      />,
    );

    expect(screen.getByText("Requesting provisioning...")).toBeInTheDocument();
  });

  it("shows only the first line of a creation error inline", () => {
    render(
      <AccountSettings
        cloudWorkspace={controls({
          provisioningError:
            "[provider_error] Create rejected\nHTTP 502 · Correlation ID: corr-1",
        })}
      />,
    );

    const alert = screen.getByRole("alert");
    expect(alert).toHaveTextContent("[provider_error] Create rejected");
    expect(alert).not.toHaveTextContent("corr-1");
  });

  it("hides Repair while the cloud workspace is healthy", () => {
    render(
      <AccountSettings
        cloudWorkspace={controls({ instanceStatus: statusWith("ready") })}
      />,
    );

    expect(
      screen.queryByRole("button", { name: "Repair" }),
    ).not.toBeInTheDocument();
    expect(
      screen.getByRole("button", { name: "Delete cloud workspace" }),
    ).toBeInTheDocument();
  });

  it("offers Repair when an issue is detected", async () => {
    const cloudWorkspace = controls({ instanceStatus: statusWith("degraded") });
    render(<AccountSettings cloudWorkspace={cloudWorkspace} />);

    await userEvent.click(screen.getByRole("button", { name: "Repair" }));

    expect(cloudWorkspace.onRepair).toHaveBeenCalledOnce();
  });

  it("confirms before deleting the cloud workspace", async () => {
    const cloudWorkspace = controls({ instanceStatus: statusWith("ready") });
    render(<AccountSettings cloudWorkspace={cloudWorkspace} />);

    await userEvent.click(
      screen.getByRole("button", { name: "Delete cloud workspace" }),
    );
    expect(cloudWorkspace.onDelete).not.toHaveBeenCalled();

    const dialog = screen.getByRole("dialog");
    expect(dialog).toHaveTextContent(/all data/i);
    await userEvent.click(
      within(dialog).getByRole("button", { name: "Delete" }),
    );

    expect(cloudWorkspace.onDelete).toHaveBeenCalledOnce();
  });

  it("shows workspace count and disk usage against the quota", () => {
    render(
      <AccountSettings
        cloudWorkspace={controls({
          instanceStatus: statusWith("ready"),
          usage: {
            repository_count: 3,
            workspace_count: 7,
            disk_used_bytes: 1.8 * 1024 ** 3,
          },
        })}
      />,
    );

    expect(
      screen.getByText("7 workspaces across 3 repositories"),
    ).toBeInTheDocument();
    expect(screen.getByText("1.8 GB of 5 GB")).toBeInTheDocument();
    expect(
      screen.getByRole("progressbar", { name: "Disk usage" }),
    ).toHaveAttribute("aria-valuenow", "36");
  });

  it("shows the error when usage cannot be loaded", () => {
    render(
      <AccountSettings
        cloudWorkspace={controls({
          instanceStatus: statusWith("ready"),
          usageError: "[http_502] exec failed",
        })}
      />,
    );

    expect(
      screen.getByText("Couldn't load usage: [http_502] exec failed"),
    ).toBeInTheDocument();
  });

  it("shows a signing-in state while a deep-link token is exchanged", () => {
    useAuthStore.setState({ user: null, signingIn: true });
    render(<AccountSettings />);

    expect(screen.getByRole("status")).toHaveTextContent("Signing you in…");
    expect(
      screen.queryByRole("button", { name: "Sign in with Browser" }),
    ).toBeNull();
  });

  it("shows why the last sign-in failed", () => {
    useAuthStore.setState({ user: null, signInError: "Token expired" });
    render(<AccountSettings />);

    expect(screen.getByRole("alert")).toHaveTextContent(
      "Sign-in failed: Token expired",
    );
  });
});
