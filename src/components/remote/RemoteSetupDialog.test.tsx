import userEvent from "@testing-library/user-event";
import { describe, expect, it, vi } from "vitest";
import { render, screen } from "../../../test/test-utils";
import { RemoteSetupDialog } from "./RemoteSetupDialog";

function baseProps() {
  return {
    open: true,
    onOpenChange: vi.fn(),
    sshConfigAliasSuggestions: ["prod-box"],
    instanceStatus: null,
    onProvisionManaged: vi.fn().mockResolvedValue(undefined),
    onWake: vi.fn().mockResolvedValue(undefined),
    onReprovision: vi.fn().mockResolvedValue(undefined),
    onDeleteInstance: vi.fn().mockResolvedValue(undefined),
    onRevokeKey: vi.fn().mockResolvedValue(undefined),
    onRegisterUserManaged: vi.fn().mockResolvedValue(undefined),
  };
}

describe("RemoteSetupDialog", () => {
  it("presents the two-choice entry point", async () => {
    render(<RemoteSetupDialog {...baseProps()} />);
    expect(
      await screen.findByRole("button", {
        name: /Treq-managed cloud workspace/,
      }),
    ).toBeInTheDocument();
    expect(
      screen.getByRole("button", { name: /Your own VM/ }),
    ).toBeInTheDocument();
  });

  it("opens straight on the managed workspace when user-managed SSH is not allowed", async () => {
    render(<RemoteSetupDialog {...baseProps()} allowUserManaged={false} />);
    expect(
      await screen.findByRole("button", { name: "Create cloud workspace" }),
    ).toBeInTheDocument();
    expect(
      screen.queryByRole("button", { name: /Your own VM/ }),
    ).not.toBeInTheDocument();
    expect(
      screen.queryByRole("button", { name: "Back" }),
    ).not.toBeInTheDocument();
  });

  it("provisions one managed cloud workspace without region, size, or SSH identity", async () => {
    const user = userEvent.setup();
    const props = baseProps();
    render(<RemoteSetupDialog {...props} />);

    await user.click(
      await screen.findByRole("button", {
        name: /Treq-managed cloud workspace/,
      }),
    );

    expect(screen.queryByLabelText("Region")).not.toBeInTheDocument();
    expect(screen.queryByLabelText("Size")).not.toBeInTheDocument();
    expect(screen.queryByLabelText("SSH identity")).not.toBeInTheDocument();

    await user.click(
      screen.getByRole("button", { name: "Create cloud workspace" }),
    );
    expect(props.onProvisionManaged).toHaveBeenCalledWith();
  });

  it("retries creation when the failed record has no provider resource", async () => {
    const user = userEvent.setup();
    const props = {
      ...baseProps(),
      instanceStatus: {
        instance: {
          instance_id: "instance-1",
          status: "failed",
          provider_resource_id: null,
          generation: 0,
        },
        endpoint: null,
      } as never,
    };
    render(<RemoteSetupDialog {...props} />);

    await user.click(
      await screen.findByRole("button", {
        name: /Treq-managed cloud workspace/,
      }),
    );
    await user.click(
      screen.getByRole("button", { name: "Retry cloud workspace creation" }),
    );

    expect(props.onProvisionManaged).toHaveBeenCalledOnce();
    expect(
      screen.queryByRole("button", { name: "Repair" }),
    ).not.toBeInTheDocument();
  });

  it("shows the first line of a structured server error inline", async () => {
    const user = userEvent.setup();
    const props = {
      ...baseProps(),
      provisioningError:
        "[invalid_request] Bad VM name\nHTTP 400 · Correlation ID: corr-123",
    };
    render(<RemoteSetupDialog {...props} />);

    await user.click(
      await screen.findByRole("button", {
        name: /Treq-managed cloud workspace/,
      }),
    );

    expect(screen.getByRole("alert")).toHaveTextContent(
      "[invalid_request] Bad VM name",
    );
  });

  it("requires host-trust confirmation before registering a user-managed endpoint", async () => {
    const user = userEvent.setup();
    const props = baseProps();
    render(<RemoteSetupDialog {...props} />);

    await user.click(
      await screen.findByRole("button", { name: /Your own VM/ }),
    );

    await user.type(await screen.findByLabelText("Display name"), "My box");
    await user.type(
      screen.getByLabelText("Hostname or IP address"),
      "203.0.113.4",
    );
    await user.type(screen.getByLabelText("Username"), "dev");
    await user.type(
      screen.getByLabelText("Expected host-key fingerprint"),
      "SHA256:deadbeef",
    );
    await user.type(
      screen.getByLabelText("Auth identity reference"),
      "/home/me/.ssh/id_ed25519",
    );

    await user.click(screen.getByRole("button", { name: "Continue" }));

    expect(props.onRegisterUserManaged).not.toHaveBeenCalled();
    const confirmation = await screen.findByTestId("host-trust-confirmation");
    expect(confirmation).toHaveTextContent("203.0.113.4");
    expect(confirmation).toHaveTextContent("SHA256:deadbeef");

    await user.click(screen.getByRole("button", { name: "Trust and connect" }));
    expect(props.onRegisterUserManaged).toHaveBeenCalledWith(
      expect.objectContaining({
        display_name: "My box",
        hostname: "203.0.113.4",
        host_key_fingerprint: "SHA256:deadbeef",
        alias: null,
      }),
    );
  });
});
