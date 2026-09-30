import userEvent from "@testing-library/user-event";
import { afterEach, describe, expect, it, vi } from "vitest";
import { RemoteAgentScreen } from "./RemoteAgentScreen";
import type { SshEndpoint } from "../lib/api-types-remote";
import * as remoteDispatch from "../lib/remote-dispatch";
import { remoteActionKeys } from "../lib/remote-idempotency";
import { render, screen } from "../../test/test-utils";

const ENDPOINT: SshEndpoint = {
  id: "endpoint-1",
  instance_id: "inst-1",
  source: { type: "managed", provider: "fly_sprites", generation: 1 },
  hostname: "inst-1.example",
  port: 22,
  username: "treq",
  host_keys: [],
  authentication: { type: "certificate", key_reference: "keystore:device" },
};

function mockStatus(running: boolean) {
  vi.spyOn(remoteDispatch, "dispatchOverSsh").mockImplementation(
    async (_endpoint, request) => {
      if (request.kind === "AgentStatus") {
        return {
          workspace: "ws",
          running,
          agent: running ? "claude" : null,
          pid: running ? 42 : null,
          started_at: null,
          should_refresh: false,
        } as never;
      }
      return "" as never;
    },
  );
}

afterEach(() => {
  vi.restoreAllMocks();
  remoteActionKeys.clear();
});

describe("RemoteAgentScreen", () => {
  it("keeps an unconfirmed start's key when the screen remounts", async () => {
    const user = userEvent.setup();
    mockStatus(false);
    const mutation = vi
      .spyOn(remoteDispatch, "dispatchMutationOverSsh")
      .mockResolvedValue({ status: "ambiguous", reason: "connection reset" });
    const start = async () => {
      await user.type(
        await screen.findByPlaceholderText("Prompt for the agent"),
        "fix it",
      );
      await user.click(screen.getByRole("button", { name: "Start agent" }));
    };

    const first = render(
      <RemoteAgentScreen endpoint={ENDPOINT} repo="/r" workspace="ws" />,
    );
    await start();
    await vi.waitFor(() => expect(mutation).toHaveBeenCalledTimes(1));
    // RemoteConnectPanel remounts the repo view on resume and reconnect.
    first.unmount();
    render(<RemoteAgentScreen endpoint={ENDPOINT} repo="/r" workspace="ws" />);
    await start();

    await vi.waitFor(() => expect(mutation).toHaveBeenCalledTimes(2));
    const keys = mutation.mock.calls.map(
      ([, request]) => (request as { idempotency_key: string }).idempotency_key,
    );
    expect(keys[1]).toBe(keys[0]);
  });

  it("retries an unconfirmed start with the same idempotency key", async () => {
    const user = userEvent.setup();
    mockStatus(false);
    const mutation = vi
      .spyOn(remoteDispatch, "dispatchMutationOverSsh")
      .mockResolvedValue({ status: "ambiguous", reason: "connection reset" });

    render(<RemoteAgentScreen endpoint={ENDPOINT} repo="/r" workspace="ws" />);
    await user.type(
      await screen.findByPlaceholderText("Prompt for the agent"),
      "fix it",
    );
    await user.click(screen.getByRole("button", { name: "Start agent" }));
    expect(
      await screen.findByText(/Could not confirm the agent started/),
    ).toBeInTheDocument();
    await user.click(screen.getByRole("button", { name: "Start agent" }));

    await vi.waitFor(() => expect(mutation).toHaveBeenCalledTimes(2));
    const keys = mutation.mock.calls.map(
      ([, request]) => (request as { idempotency_key: string }).idempotency_key,
    );
    expect(keys[0]).toBe(keys[1]);
  });

  it("asks for confirmation before stopping and reports an unconfirmed stop", async () => {
    const user = userEvent.setup();
    mockStatus(true);
    const mutation = vi
      .spyOn(remoteDispatch, "dispatchMutationOverSsh")
      .mockResolvedValue({ status: "ambiguous", reason: "connection reset" });

    render(<RemoteAgentScreen endpoint={ENDPOINT} repo="/r" workspace="ws" />);
    await user.click(await screen.findByRole("button", { name: "Stop agent" }));
    expect(mutation).not.toHaveBeenCalled();

    await user.click(screen.getByRole("button", { name: "Confirm stop" }));

    await vi.waitFor(() =>
      expect(mutation).toHaveBeenCalledWith(ENDPOINT, {
        kind: "AgentStop",
        repo: "/r",
        workspace: "ws",
      }),
    );
    expect(
      await screen.findByText(/Could not confirm the change applied/),
    ).toBeInTheDocument();
  });
});
