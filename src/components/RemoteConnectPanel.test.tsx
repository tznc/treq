import { readFileSync } from "node:fs";
import { resolve } from "node:path";
import userEvent from "@testing-library/user-event";
import type { User } from "@supabase/supabase-js";
import { act } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { RemoteConnectPanel } from "./RemoteConnectPanel";
import { SECURE_STORAGE_UNAVAILABLE_PREFIX } from "../hooks/useMobileRemoteConnection";
import * as api from "../lib/api";
import * as apiExtra from "../lib/api-extra";
import {
  DEVICE_KEYSTORE_KEY_REFERENCE,
  type InstanceStatusResponse,
  type IssueCertificateResponse,
  type SshEndpoint,
} from "../lib/api-types-remote";
import * as certLifecycle from "../lib/remote-cert-lifecycle";
import * as controlPlane from "../lib/remote-control-plane";
import * as remoteDispatch from "../lib/remote-dispatch";
import * as remoteEndpoints from "../lib/remote-endpoints";
import * as remoteRepository from "../lib/remote-repository";
import { MOBILE_SESSION_KEY, saveMobileSession } from "../lib/mobile-session";
import { defaultAuthState, useAuthStore } from "../stores/authStore";
import { useRemoteCutoffStore } from "../stores/remoteCutoffStore";
import { render, screen } from "../../test/test-utils";

const MANAGED_ENDPOINT: SshEndpoint = {
  id: "endpoint-1",
  instance_id: "inst-1",
  source: { type: "managed", provider: "fly_sprites", generation: 1 },
  hostname: "inst-1.example",
  port: 22,
  username: "treq",
  host_keys: [],
  authentication: { type: "certificate", key_reference: "key-1" },
};

function readyStatus(generation = 1): InstanceStatusResponse {
  return {
    instance: { instance_id: "inst-1", status: "ready", generation },
    endpoint: MANAGED_ENDPOINT,
  } as never;
}

function issued(
  expiresAt = "2099-01-01T00:00:00Z",
  certificate = "ssh-ed25519-cert-v01@openssh.com AAAA issued",
): IssueCertificateResponse {
  return {
    certificate,
    serial: "1",
    expires_at: expiresAt,
    endpoint: MANAGED_ENDPOINT,
  };
}

/** Mocks the control plane and device key for a successful managed connect. */
function mockManagedControlPlane() {
  vi.spyOn(api, "ensureMobileDeviceKey").mockResolvedValue({
    public_key: "ssh-ed25519 AAAA device",
    fingerprint_sha256: "SHA256:device",
  });
  const register = vi
    .spyOn(controlPlane, "registerClientKey")
    .mockResolvedValue({
      id: "key-1",
      algorithm: "ssh-ed25519",
      fingerprint_sha256: "SHA256:device",
      comment: "treq-mobile-device",
      created_at: "2024-01-01T00:00:00Z",
      revoked_at: null,
    });
  const status = vi
    .spyOn(controlPlane, "getInstanceStatus")
    .mockResolvedValue(readyStatus());
  const issue = vi
    .spyOn(controlPlane, "issueCertificate")
    .mockResolvedValue(issued());
  // Renewal timers are covered by `remote-cert-lifecycle.test.ts`; here
  // only that the panel starts and stops one matters.
  const stopRenewal = vi.fn();
  const startRenewal = vi
    .spyOn(certLifecycle, "startManagedCertificateRenewal")
    .mockReturnValue({ stop: stopRenewal } as never);
  return { register, status, issue, startRenewal, stopRenewal };
}

function signIn() {
  useAuthStore.setState({
    user: { id: "user-1", email: "me@example.com" } as User,
    loading: false,
  });
}

function setVisibility(state: "hidden" | "visible") {
  Object.defineProperty(document, "visibilityState", {
    configurable: true,
    get: () => state,
  });
  document.dispatchEvent(new Event("visibilitychange"));
}

beforeEach(() => {
  localStorage.clear();
  useAuthStore.setState({ ...defaultAuthState });
  useRemoteCutoffStore.setState({ cutoffs: {} });
  vi.spyOn(remoteEndpoints, "listUserManagedEndpoints").mockResolvedValue([]);
  vi.spyOn(
    remoteRepository,
    "listSavedRepositoriesForEndpoint",
  ).mockResolvedValue([]);
  vi.spyOn(remoteRepository, "upsertSavedRemoteRepository").mockResolvedValue(
    {} as never,
  );
});

afterEach(() => {
  vi.restoreAllMocks();
  localStorage.clear();
});

describe("SECURE_STORAGE_UNAVAILABLE_PREFIX", () => {
  it("stays byte-for-byte in sync with the Rust constant of the same name", () => {
    // `SECURE_STORAGE_UNAVAILABLE_PREFIX` is hand-duplicated on the TS side
    // because `ensure_mobile_device_key`'s Tauri command returns a plain
    // `Result<_, String>` (shared with other `remote_*` commands - see the
    // doc comment on the Rust constant), so there's no generated binding to
    // pull the value from. This test is the enforcement that would
    // otherwise be missing: it fails loudly if either literal changes
    // without the other, instead of silently breaking the
    // "set up biometrics" UI state.
    const rustSourcePath = resolve(
      process.cwd(),
      "src-tauri/src/core/remote_device_key.rs",
    );
    const rustSource = readFileSync(rustSourcePath, "utf-8");
    const match = rustSource.match(
      /pub const SECURE_STORAGE_UNAVAILABLE_PREFIX: &str = "([^"]*)";/,
    );
    expect(
      match,
      "could not find `SECURE_STORAGE_UNAVAILABLE_PREFIX` const in remote_device_key.rs - has it moved or been renamed?",
    ).not.toBeNull();
    expect(SECURE_STORAGE_UNAVAILABLE_PREFIX).toBe(match?.[1]);
  });
});

describe("DEVICE_KEYSTORE_KEY_REFERENCE", () => {
  it("matches the reserved reference the Rust transport resolves", () => {
    const rustSource = readFileSync(
      resolve(process.cwd(), "src-tauri/src/core/remote_ssh_transport.rs"),
      "utf-8",
    );
    const match = rustSource.match(
      /pub const DEVICE_KEYSTORE_KEY_REFERENCE: &str = "([^"]*)";/,
    );
    expect(match).not.toBeNull();
    expect(DEVICE_KEYSTORE_KEY_REFERENCE).toBe(match?.[1]);
  });
});

describe("RemoteConnectPanel", () => {
  it("dispatches over SSH with the device keystore key and the issued certificate", async () => {
    const user = userEvent.setup();
    signIn();
    mockManagedControlPlane();
    // `exists: false` keeps the repository screen (and its own dispatches)
    // out of this test; only the endpoint shape matters here.
    const dispatch = vi
      .spyOn(remoteDispatch, "dispatchOverSsh")
      .mockResolvedValue({
        host: "inst-1.example",
        path: "/home/treq/repo",
        exists: false,
        is_repo: false,
        needs_clone: true,
      });

    render(<RemoteConnectPanel />);
    await user.click(
      screen.getByRole("button", { name: "Connect to managed instance" }),
    );
    await user.type(
      await screen.findByPlaceholderText("Repository path on the instance"),
      "/home/treq/repo",
    );
    await user.click(
      screen.getByRole("button", { name: "Inspect repository" }),
    );

    await vi.waitFor(() => expect(dispatch).toHaveBeenCalled());
    expect(dispatch).toHaveBeenCalledWith(
      expect.objectContaining({
        authentication: {
          type: "certificate",
          key_reference: DEVICE_KEYSTORE_KEY_REFERENCE,
          certificate: "ssh-ed25519-cert-v01@openssh.com AAAA issued",
        },
      }),
      expect.anything(),
    );
  });

  it("shows a dedicated state when secure storage / biometrics isn't set up", async () => {
    const user = userEvent.setup();
    signIn();
    mockManagedControlPlane();
    vi.spyOn(api, "ensureMobileDeviceKey").mockRejectedValue(
      new Error(
        "secure_storage_unavailable:Biometrics are not set up on this device; the device key cannot be stored securely.",
      ),
    );

    render(<RemoteConnectPanel />);
    await user.click(
      screen.getByRole("button", { name: "Connect to managed instance" }),
    );

    const alert = await screen.findByRole("alert");
    expect(alert).toHaveTextContent(
      "Secure storage isn't set up on this device",
    );
    expect(alert).toHaveTextContent("Biometrics are not set up on this device");
    // The generic connect button is replaced by the dedicated retry action.
    expect(
      screen.queryByRole("button", { name: "Connect to managed instance" }),
    ).not.toBeInTheDocument();
    expect(
      screen.getByRole("button", { name: "Try again" }),
    ).toBeInTheDocument();
  });

  it("shows a generic error for non-secure-storage failures", async () => {
    const user = userEvent.setup();
    signIn();
    mockManagedControlPlane();
    vi.spyOn(api, "ensureMobileDeviceKey").mockRejectedValue(
      new Error("network unreachable"),
    );

    render(<RemoteConnectPanel />);
    await user.click(
      screen.getByRole("button", { name: "Connect to managed instance" }),
    );

    expect(await screen.findByText("network unreachable")).toBeInTheDocument();
    expect(screen.queryByRole("alert")).not.toBeInTheDocument();
    expect(
      screen.getByRole("button", { name: "Connect to managed instance" }),
    ).toBeInTheDocument();
  });

  it("needs a signed-in account for the managed instance", () => {
    render(<RemoteConnectPanel />);
    expect(
      screen.getByRole("button", { name: "Connect to managed instance" }),
    ).toBeDisabled();
    expect(
      screen.getByText("Sign in to connect to your Treq-managed instance."),
    ).toBeInTheDocument();
  });

  it("connects to a user-managed endpoint and lists its known repositories", async () => {
    const user = userEvent.setup();
    vi.mocked(remoteEndpoints.listUserManagedEndpoints).mockResolvedValue([
      {
        id: "home-box",
        display_name: "Home box",
        hostname: "box.local",
        port: 2222,
        username: "me",
        host_key_fingerprint: "SHA256:host",
        auth_identity_reference: "/home/me/.ssh/id_ed25519.pub",
        alias: null,
        created_at: "2024-01-01T00:00:00Z",
      },
    ]);
    vi.mocked(
      remoteRepository.listSavedRepositoriesForEndpoint,
    ).mockResolvedValue([
      {
        id: "repo-1",
        endpoint_id: "home-box",
        endpoint_generation: 0,
        canonical_remote_path: "/srv/app",
        display_name: "app",
        last_successful_trust_validation: null,
      },
    ]);
    const dispatch = vi
      .spyOn(remoteDispatch, "dispatchOverSsh")
      .mockResolvedValue({ exists: false, is_repo: false } as never);

    render(<RemoteConnectPanel />);
    await user.click(await screen.findByRole("button", { name: /Home box/ }));
    await user.click(await screen.findByRole("button", { name: /\/srv\/app/ }));

    await vi.waitFor(() => expect(dispatch).toHaveBeenCalled());
    expect(dispatch).toHaveBeenCalledWith(
      expect.objectContaining({
        id: "home-box",
        hostname: "box.local",
        authentication: expect.objectContaining({
          type: "public_key",
          key_reference: "/home/me/.ssh/id_ed25519.pub",
        }),
      }),
      { kind: "ProbeRepo", repo: "/srv/app" },
    );
  });

  it("remembers the endpoint, repository and screen but never the certificate", async () => {
    const user = userEvent.setup();
    signIn();
    mockManagedControlPlane();
    vi.spyOn(remoteDispatch, "dispatchOverSsh").mockImplementation(
      async (_endpoint, request) => {
        if (request.kind === "ProbeRepo") {
          return { exists: true, is_repo: true } as never;
        }
        return [] as never;
      },
    );

    render(<RemoteConnectPanel />);
    await user.click(
      screen.getByRole("button", { name: "Connect to managed instance" }),
    );
    await user.type(
      await screen.findByPlaceholderText("Repository path on the instance"),
      "/home/treq/repo",
    );
    await user.click(
      screen.getByRole("button", { name: "Inspect repository" }),
    );
    await screen.findByText("Workspaces");

    await vi.waitFor(() => {
      const stored = localStorage.getItem(MOBILE_SESSION_KEY) ?? "null";
      expect(JSON.parse(stored)).toEqual({
        endpoint: { kind: "managed" },
        repoPath: "/home/treq/repo",
        screen: null,
      });
    });
    const stored = localStorage.getItem(MOBILE_SESSION_KEY) ?? "";
    expect(stored).not.toContain("cert");
    expect(stored).not.toContain("inst-1.example");
  });

  it("restores a remembered session from fresh remote state on launch", async () => {
    signIn();
    const { status, issue } = mockManagedControlPlane();
    saveMobileSession({
      endpoint: { kind: "managed" },
      repoPath: "/home/treq/repo",
      screen: { name: "agent", workspace: "feat-x" },
    });
    const dispatch = vi
      .spyOn(remoteDispatch, "dispatchOverSsh")
      .mockImplementation(async (_endpoint, request) => {
        if (request.kind === "ProbeRepo") {
          return { exists: true, is_repo: true } as never;
        }
        if (request.kind === "AgentStatus") {
          return { running: false, workspace: "feat-x" } as never;
        }
        return [] as never;
      });

    render(<RemoteConnectPanel />);

    // The agent screen asks the VM for the agent's state rather than
    // assuming anything about it.
    expect(await screen.findByText("Agent · feat-x")).toBeInTheDocument();
    expect(status).toHaveBeenCalled();
    expect(issue).toHaveBeenCalledTimes(1);
    await vi.waitFor(() =>
      expect(dispatch).toHaveBeenCalledWith(
        expect.objectContaining({
          authentication: expect.objectContaining({
            key_reference: DEVICE_KEYSTORE_KEY_REFERENCE,
          }),
        }),
        { kind: "AgentStatus", repo: "/home/treq/repo", workspace: "feat-x" },
      ),
    );
  });

  it("gets a new certificate on resume once the old one is due", async () => {
    const user = userEvent.setup();
    signIn();
    const { status, issue } = mockManagedControlPlane();
    // Already past its renewal point, as after a long suspension.
    issue.mockResolvedValueOnce(issued("2000-01-01T00:00:00Z"));

    render(<RemoteConnectPanel />);
    await user.click(
      screen.getByRole("button", { name: "Connect to managed instance" }),
    );
    await screen.findByText(/Connected to inst-1.example/);
    expect(issue).toHaveBeenCalledTimes(1);
    const statusCalls = status.mock.calls.length;

    await act(async () => {
      setVisibility("hidden");
      setVisibility("visible");
    });

    await vi.waitFor(() => expect(issue).toHaveBeenCalledTimes(2));
    expect(status.mock.calls.length).toBeGreaterThan(statusCalls);
  });

  it("re-checks the instance on resume but keeps a fresh certificate", async () => {
    const user = userEvent.setup();
    signIn();
    const { status, issue } = mockManagedControlPlane();

    render(<RemoteConnectPanel />);
    await user.click(
      screen.getByRole("button", { name: "Connect to managed instance" }),
    );
    await screen.findByText(/Connected to inst-1.example/);
    const statusCalls = status.mock.calls.length;

    await act(async () => {
      setVisibility("hidden");
      setVisibility("visible");
    });

    await vi.waitFor(() =>
      expect(status.mock.calls.length).toBeGreaterThan(statusCalls),
    );
    expect(issue).toHaveBeenCalledTimes(1);
  });

  it("blocks the repository view on a credential cutoff until reauthentication succeeds", async () => {
    const user = userEvent.setup();
    signIn();
    const { issue } = mockManagedControlPlane();
    const clearCutoff = vi
      .spyOn(apiExtra, "remoteClearCutoff")
      .mockResolvedValue(undefined);
    vi.spyOn(remoteDispatch, "dispatchOverSsh").mockImplementation(
      async (_endpoint, request) => {
        if (request.kind === "ProbeRepo") {
          return { exists: true, is_repo: true } as never;
        }
        return [] as never;
      },
    );

    render(<RemoteConnectPanel />);
    await user.click(
      screen.getByRole("button", { name: "Connect to managed instance" }),
    );
    await user.type(
      await screen.findByPlaceholderText("Repository path on the instance"),
      "/home/treq/repo",
    );
    await user.click(
      screen.getByRole("button", { name: "Inspect repository" }),
    );
    await screen.findByText("Workspaces");

    act(() => {
      useRemoteCutoffStore
        .getState()
        .recordCutoff(MANAGED_ENDPOINT.id, "key_revoked");
    });

    expect(await screen.findByText("Remote access is blocked")).toBeVisible();
    expect(screen.queryByText("Workspaces")).not.toBeInTheDocument();
    expect(screen.getByTestId("remote-status-banner")).toHaveAttribute(
      "data-state",
      "cutoff",
    );

    await user.click(screen.getByRole("button", { name: "Reauthenticate" }));

    await vi.waitFor(() =>
      expect(clearCutoff).toHaveBeenCalledWith(MANAGED_ENDPOINT.id),
    );
    expect(issue).toHaveBeenCalledTimes(2);
    expect(await screen.findByText("Workspaces")).toBeInTheDocument();
  });
});
