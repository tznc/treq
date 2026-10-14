import * as React from "react";
import userEvent from "@testing-library/user-event";
import { expect, it, vi } from "vitest";
import { Dashboard } from "../../../src/components/Dashboard";
import type {
  InstanceStatusResponse,
  ManagedInstanceRecord,
  SshEndpoint,
} from "../../../src/lib/api-types-remote";
import type { TreqCommandRequest } from "../../../src/lib/remote-dispatch";
import { render, screen, within } from "../../../test/test-utils";
import { createTestRepo, openRepo } from "../../../test/utils";
import { captureDocument } from "../capture";
import { sendGpuiAction } from "../../../test/gpui-sidebar";

// Cloud workspace lifecycle lives behind the `remote-instance` Supabase Edge
// Function, which the desktop harness cannot reach, usage is read from the
// machine over managed SSH, and the account page needs a signed-in user.
// Only those three boundaries are stubbed; the real
// repo, Dashboard handlers, settings page, and `remote-control-plane` error
// decoding all run. `server` is mutated by the spec to walk the flow through
// each state the edge function can report.
const { server, auth } = vi.hoisted(() => ({
  server: {
    status: { instance: null, endpoint: null } as InstanceStatusResponse,
    nextEnsure: "fail" as "fail" | "defer",
    releaseEnsure: null as null | (() => void),
    actions: [] as string[],
    sshRequests: [] as TreqCommandRequest[],
  },
  auth: {
    user: {
      id: "user-qa",
      email: "dev@example.com",
      user_metadata: { full_name: "Dev User" },
    },
    session: { access_token: "token" },
    loading: false,
    subscription: { plan: "pro", status: "active" },
    signIn: () => {},
    signOut: () => {},
  },
}));

function cloudInstance(
  overrides: Partial<ManagedInstanceRecord>,
): ManagedInstanceRecord {
  return {
    instance_id: "inst-qa",
    owner_user_id: "user-qa",
    provider_kind: "fly_sprites",
    provider_resource_id: "treq-user-qa",
    region: "us_east",
    size_preset: "small",
    status: "provisioning",
    generation: 1,
    endpoint_id: null,
    image_manifest_version: 1,
    created_at: "2026-09-26T00:00:00Z",
    ready_at: null,
    disk_quota_gb: 20,
    vcpu_quota: 2,
    ram_quota_gb: 4,
    ...overrides,
  };
}

const ENDPOINT: SshEndpoint = {
  id: "ep-qa",
  instance_id: "inst-qa",
  source: { type: "managed", provider: "fly_sprites", generation: 1 },
  hostname: "vm-qa.treq.dev",
  port: 22,
  username: "treq",
  host_keys: [],
  authentication: { type: "public_key", key_reference: "key-qa" },
};

vi.mock("../../../src/lib/remote-dispatch", async (importOriginal) => {
  const actual =
    await importOriginal<typeof import("../../../src/lib/remote-dispatch")>();
  return {
    ...actual,
    dispatchOverSsh: vi.fn(async (_endpoint, request: TreqCommandRequest) => {
      server.sshRequests.push(request);
      return {
        repository_count: 3,
        workspace_count: 7,
        disk_used_bytes: 1.8 * 1024 ** 3,
      };
    }),
  };
});

// Managed SSH authenticates with a key from the user's ~/.ssh. Stub the two
// reads so the flow doesn't depend on the machine running the spec.
vi.mock("../../../src/lib/api", async (importOriginal) => {
  const actual = await importOriginal<typeof import("../../../src/lib/api")>();
  return {
    ...actual,
    listLocalSshIdentities: vi.fn(async () => [
      {
        reference: "/home/qa/.ssh/id_ed25519.pub",
        label: "id_ed25519",
        fingerprint_sha256: "SHA256:qa",
        algorithm: "ssh-ed25519",
      },
    ]),
    readLocalSshPublicKey: vi.fn(async () => "ssh-ed25519 AAAAqa qa@treq"),
  };
});

vi.mock("../../../src/stores/authStore", async (importOriginal) => {
  const actual =
    await importOriginal<typeof import("../../../src/stores/authStore")>();
  const { createAuthStoreMock } = await import(
    "../../../test/mocks/zustandAuthStore"
  );
  return { ...actual, useAuthStore: createAuthStoreMock(auth) };
});

vi.mock("../../../src/lib/supabase", () => ({
  supabase: {
    rpc: vi.fn(async () => ({ data: [], error: null })),
    from: () => ({
      select: () => Promise.resolve({ data: [], error: null }),
    }),
    functions: {
      invoke: vi.fn(
        async (_fn: string, { body }: { body: { action: string } }) => {
          server.actions.push(body.action);
          if (body.action === "status") {
            return { data: server.status, error: null };
          }
          if (body.action === "register_client_key") {
            return {
              data: {
                operation_id: "op-key",
                status: "succeeded",
                key: {
                  id: "key-qa",
                  algorithm: "ssh-ed25519",
                  fingerprint_sha256: "SHA256:qa",
                  comment: "treq-desktop",
                  created_at: "2026-09-26T00:00:00Z",
                  revoked_at: null,
                },
              },
              error: null,
            };
          }
          if (body.action === "ensure" && server.nextEnsure === "fail") {
            // The control plane records the failed attempt before replying.
            server.status = {
              instance: cloudInstance({
                status: "failed",
                provider_resource_id: null,
              }),
              endpoint: null,
            };
            return {
              data: null,
              error: {
                message: "Edge Function returned a non-2xx status code",
                context: new Response(
                  JSON.stringify({
                    error: "Provider rejected the create request",
                    code: "provider_error",
                    correlation_id: "corr-7f3a",
                  }),
                  { status: 502 },
                ),
              },
            };
          }
          if (body.action === "ensure") {
            await new Promise<void>((resolve) => {
              server.releaseEnsure = resolve;
            });
            server.status = {
              instance: cloudInstance({ status: "bootstrapping" }),
              endpoint: null,
            };
            return {
              data: { operation_id: "op-1", status: "running" },
              error: null,
            };
          }
          return { data: null, error: null };
        },
      ),
    },
    auth: {
      getSession: vi
        .fn()
        .mockResolvedValue({ data: { session: null }, error: null }),
      onAuthStateChange: vi.fn().mockReturnValue({
        data: { subscription: { unsubscribe: vi.fn() } },
      }),
    },
  },
  SUPABASE_URL: "http://localhost:54321",
  SUPABASE_ANON_KEY: "anon",
  WEB_URL: "http://localhost:3000",
}));

it("captures creating and managing a cloud workspace from account settings", async () => {
  const { repoPath } = createTestRepo(false);
  openRepo(repoPath);
  const user = userEvent.setup();
  render(<Dashboard />);

  sendGpuiAction({ kind: "openSettings" });
  await user.click(await screen.findByRole("tab", { name: /account/i }));
  await screen.findByRole("button", { name: "Create cloud workspace" });
  expect(screen.queryByText(/Sprite/)).toBeNull();
  await captureDocument(document, {
    name: "remote-cloud-workspace-01-account-empty",
    expectations: [
      "The Account settings tab shows a 'Cloud workspace' card below the Subscription card.",
      "The card has a short description and a full-width primary 'Create cloud workspace' button; no dialog is open.",
      "The word 'Sprite' does not appear anywhere.",
    ],
  });

  // First attempt: the provider rejects the create call.
  await user.click(
    screen.getByRole("button", { name: "Create cloud workspace" }),
  );
  const alert = await screen.findByRole("alert");
  expect(alert.textContent).toBe(
    "[provider_error] Provider rejected the create request",
  );
  await screen.findByText("Couldn't create cloud workspace");
  await screen.findByRole("button", {
    name: "Retry cloud workspace creation",
  });
  await captureDocument(document, {
    name: "remote-cloud-workspace-02-create-error",
    expectations: [
      "An error toast titled 'Couldn't create cloud workspace' shows the provider error and the 'HTTP 502 · Correlation ID: corr-7f3a' detail.",
      "Inside the card, a single red line reads '[provider_error] Provider rejected the create request' with no correlation line.",
      "The card button now reads 'Retry cloud workspace creation'.",
    ],
  });

  // Second attempt: hold the request open to capture the in-flight state.
  server.nextEnsure = "defer";
  await user.click(
    screen.getByRole("button", { name: "Retry cloud workspace creation" }),
  );
  const creating = await screen.findByRole("button", { name: "Creating..." });
  expect(creating).toBeDisabled();
  await screen.findByText("Requesting provisioning...");
  await captureDocument(document, {
    name: "remote-cloud-workspace-03-creating",
    expectations: [
      "The card button reads 'Creating...' with a spinner and looks disabled.",
      "A muted 'Requesting provisioning...' line is under the button and the inline error is gone.",
    ],
  });

  server.releaseEnsure?.();
  await screen.findByText("Installing Treq, JJ, Git, and agents...");
  expect(screen.queryByRole("button", { name: "Repair" })).toBeNull();
  await captureDocument(document, {
    name: "remote-cloud-workspace-04-bootstrapping",
    expectations: [
      "The card shows 'Installing Treq, JJ, Git, and agents...' as its status.",
      "Only a trash icon button sits at the right end of the card header; there is no Repair button.",
    ],
  });

  // Setup finishes; revisiting the Account tab reloads status and usage.
  server.status = {
    instance: cloudInstance({
      status: "ready",
      ready_at: "2026-09-26T00:05:00Z",
      disk_quota_gb: 5,
    }),
    endpoint: ENDPOINT,
  };
  await user.click(screen.getByRole("tab", { name: /repository/i }));
  await user.click(screen.getByRole("tab", { name: /account/i }));
  await screen.findByText("7 workspaces across 3 repositories");
  expect(screen.getByText("1.8 GB of 5 GB")).toBeTruthy();
  expect(server.sshRequests.at(-1)).toEqual({
    kind: "MachineUsage",
    root: "/home/treq",
  });
  await captureDocument(document, {
    name: "remote-cloud-workspace-05-ready-usage",
    expectations: [
      "The card status reads 'Ready' with no Repair button; only the trash icon is at the right of the header.",
      "Below the status, '7 workspaces across 3 repositories' is on the left and '1.8 GB of 5 GB' on the right.",
      "A thin usage bar under that line is filled about a third of the way.",
    ],
  });

  // An issue is detected; revisiting the Account tab reloads status.
  server.status = {
    instance: cloudInstance({ status: "degraded" }),
    endpoint: null,
  };
  await user.click(screen.getByRole("tab", { name: /repository/i }));
  await user.click(screen.getByRole("tab", { name: /account/i }));
  const repair = await screen.findByRole("button", { name: "Repair" });
  // jsdom has no layout, so the floating tooltip cannot be positioned in the
  // capture; assert its copy in the DOM instead.
  await user.hover(repair);
  expect(await screen.findByRole("tooltip")).toHaveTextContent(
    "Re-run Treq setup on this cloud workspace. Files and repositories are kept.",
  );
  await captureDocument(document, {
    name: "remote-cloud-workspace-06-repair",
    expectations: [
      "The card status reads 'Degraded'.",
      "A 'Repair' button with a wrench icon sits right-aligned in the card header, next to the trash icon.",
    ],
  });

  await user.unhover(repair);
  await user.click(
    screen.getByRole("button", { name: "Delete cloud workspace" }),
  );
  const confirm = await screen.findByRole("dialog", {
    name: "Delete cloud workspace?",
  });
  expect(server.actions).not.toContain("delete");
  await captureDocument(document, {
    name: "remote-cloud-workspace-07-delete-confirm",
    expectations: [
      "A confirmation dialog titled 'Delete cloud workspace?' warns that all data on the cloud machine is permanently deleted.",
      "The dialog has 'Cancel' and a red 'Delete' button.",
    ],
  });

  await user.click(within(confirm).getByRole("button", { name: "Delete" }));
  expect(server.actions).toContain("delete");
}, 60000);
