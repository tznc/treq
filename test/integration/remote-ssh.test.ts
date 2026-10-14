// @include-parallel
import fs from "node:fs";
import os from "node:os";
import path from "node:path";
import userEvent from "@testing-library/user-event";
import * as React from "react";
import { beforeEach, describe, expect, it, onTestFinished } from "vitest";
import { Dashboard } from "../../src/components/Dashboard";
import { listSshHosts, setSetting } from "../../src/lib/api";
import type {
  RemoteRepoProbe,
  RepositoryInspection,
} from "../../src/lib/api-types-remote";
import { dispatchLocal } from "../../src/lib/remote-dispatch";
import { listUserManagedEndpoints } from "../../src/lib/remote-endpoints";
import {
  clearLastOpenedRemoteRepository,
  listSavedRepositoriesForEndpoint,
  upsertSavedRemoteRepository,
} from "../../src/lib/remote-repository";
import {
  LOOPBACK_SSH_HOST_KEY_FINGERPRINT,
  LOOPBACK_SSH_HOSTNAME,
  TEST_SSH_PUBLIC_KEY,
} from "../loopback-ssh";
import { render, screen } from "../test-utils";
import { createTestRepo } from "../utils";

describe("remote SSH integration", () => {
  let user: ReturnType<typeof userEvent.setup>;

  beforeEach(() => {
    user = userEvent.setup();
    window.history.replaceState({}, "", "/");
  });

  it("opens the remote setup dialog from onboarding", async () => {
    render(React.createElement(Dashboard));
    await user.click(
      await screen.findByRole("button", { name: "Open via SSH" }),
    );

    expect(
      await screen.findByRole("dialog", {
        name: "Connect a remote repository",
      }),
    ).toBeTruthy();
    expect(
      screen.getByRole("button", { name: /Treq-managed cloud workspace/ }),
    ).toBeTruthy();
    expect(screen.getByRole("button", { name: /Your own VM/ })).toBeTruthy();
  });

  it.each([
    "missing-descriptor",
    "",
  ])("keeps a saved remote repository closed until reconnect and trust validation succeed (descriptor id %j)", async (descriptorId) => {
    const remoteRepository = {
      host: "devbox",
      path: "/srv/project",
      display_name: "devbox:project",
      repo_uri: "ssh://devbox/srv/project",
      inspection: {
        root: "/srv/project",
        repository_type: "jj_colocated",
        current_branch: "main",
        default_branch: "main",
        current_change_id: "change-id",
        current_commit_id: "commit-id",
        descriptor: {
          id: "ssh:devbox:/srv/project",
          location: { type: "ssh", host: "devbox", path: "/srv/project" },
          display_name: "devbox:project",
        },
      },
    };
    await setSetting(
      "last_opened_remote_repo",
      JSON.stringify(remoteRepository),
    );
    await setSetting("last_opened_remote_repo_id", descriptorId);

    render(React.createElement(Dashboard));

    expect(
      await screen.findByRole("button", { name: "Open via SSH" }),
    ).toBeTruthy();
    expect(screen.queryByText("Remote repository connected")).toBeNull();
    await setSetting("last_opened_remote_repo", "");
    await setSetting("last_opened_remote_repo_id", "");
  });

  it("lists SSH hosts as an array even when no user config is present", async () => {
    await expect(listSshHosts()).resolves.toEqual(expect.any(Array));
  });

  it("persists two endpoint-generation-scoped repositories without secrets", async () => {
    await upsertSavedRemoteRepository({
      endpoint_id: "it-ep-1",
      endpoint_generation: 7,
      remote_path: "/srv/alpha",
    });
    await upsertSavedRemoteRepository({
      endpoint_id: "it-ep-1",
      endpoint_generation: 7,
      remote_path: "/srv/beta",
    });
    await upsertSavedRemoteRepository({
      endpoint_id: "it-ep-1",
      endpoint_generation: 7,
      remote_path: "/srv/alpha/",
    });

    const listed = await listSavedRepositoriesForEndpoint("it-ep-1", 7);
    expect(listed.map((repo) => repo.canonical_remote_path).sort()).toEqual([
      "/srv/alpha",
      "/srv/beta",
    ]);
    expect(JSON.stringify(listed)).not.toMatch(
      /private_key|password|passphrase|BEGIN OPENSSH/i,
    );
  });

  it("probes, clones, inspects, and inits through typed commands", async () => {
    const dir = fs.mkdtempSync(path.join(os.tmpdir(), "treq-remote-typed-"));
    const source = path.join(dir, "source");
    fs.mkdirSync(source);
    const empty = path.join(dir, "empty");
    fs.mkdirSync(empty);

    const missingProbe = await dispatchLocal<RemoteRepoProbe>({
      kind: "ProbeRepo",
      repo: empty,
    });
    expect(missingProbe.is_repo).toBe(false);
    expect(missingProbe.needs_clone).toBe(true);

    const initialized = await dispatchLocal<RepositoryInspection>({
      kind: "InitRepo",
      repo: source,
      idempotency_key: "test-init-1",
    });
    expect(initialized.repository_type).toBeTruthy();

    const inspected = await dispatchLocal<RepositoryInspection>({
      kind: "InspectRepository",
      repo: source,
    });
    expect(inspected.root).toContain("source");

    const cloneDest = path.join(dir, "clone");
    const cloned = await dispatchLocal<RepositoryInspection>({
      kind: "CloneRepo",
      repo_url: source,
      destination: cloneDest,
      idempotency_key: "test-clone-1",
    });
    expect(cloned.root).toContain("clone");

    const cloneProbe = await dispatchLocal<RemoteRepoProbe>({
      kind: "ProbeRepo",
      repo: cloneDest,
    });
    expect(cloneProbe.is_repo).toBe(true);
  });

  function writeSshConfig(contents: string) {
    const home = fs.mkdtempSync(path.join(os.tmpdir(), "treq-alias-home-"));
    fs.mkdirSync(path.join(home, ".ssh"));
    fs.writeFileSync(path.join(home, ".ssh", "config"), contents);
    fs.writeFileSync(
      path.join(home, ".ssh", "id_ed25519.pub"),
      `${TEST_SSH_PUBLIC_KEY}\n`,
    );
    const previousHome = process.env.HOME;
    process.env.HOME = home;
    onTestFinished(() => {
      process.env.HOME = previousHome;
    });
  }

  async function startAliasRegistration() {
    render(React.createElement(Dashboard));
    await user.click(
      await screen.findByRole("button", { name: "Open via SSH" }),
    );
    await user.click(
      await screen.findByRole("button", { name: /Your own VM/ }),
    );
    await user.type(await screen.findByLabelText("Display name"), "QA box");
    await user.type(screen.getByLabelText("Username"), "treq");
    expect(await screen.findByLabelText("SSH key")).toHaveDisplayValue(
      /id_ed25519/,
    );
    await user.click(screen.getByLabelText(/Use an explicit SSH alias/));
    await user.type(screen.getByLabelText(/^Alias/), "qa-alias");
  }

  it("registers the host an alias autofills, exactly as the user confirmed it", async () => {
    writeSshConfig(
      `Host qa-alias\n  HostName ${LOOPBACK_SSH_HOSTNAME}\n  Port 2222\n`,
    );
    const { repoPath } = createTestRepo(false);
    onTestFinished(clearLastOpenedRemoteRepository);

    await startAliasRegistration();
    await user.click(
      screen.getByRole("button", { name: "Autofill from alias" }),
    );
    expect(await screen.findByDisplayValue(LOOPBACK_SSH_HOSTNAME)).toBeTruthy();
    await user.click(screen.getByRole("button", { name: "Continue" }));
    expect(
      await screen.findByTestId("host-trust-confirmation"),
    ).toHaveTextContent(`treq@${LOOPBACK_SSH_HOSTNAME}:2222`);
    await user.click(screen.getByRole("button", { name: "Trust and connect" }));

    const pathInput = await screen.findByLabelText("Remote repository path");
    const [record] = await listUserManagedEndpoints();
    expect(record).toMatchObject({
      alias: "qa-alias",
      hostname: LOOPBACK_SSH_HOSTNAME,
      port: 2222,
      host_key_fingerprint: LOOPBACK_SSH_HOST_KEY_FINGERPRINT,
      auth_identity_reference: expect.stringContaining("id_ed25519.pub"),
    });
    await user.clear(pathInput);
    await user.type(pathInput, repoPath);
    await user.click(screen.getByRole("button", { name: "Probe" }));
    await user.click(await screen.findByRole("button", { name: "Open" }));

    expect(await screen.findByTestId("show-workspace-header")).toBeTruthy();
    expect(screen.getByRole("button", { name: "Terminal" })).toBeTruthy();
  }, 20_000);

  it("pins the hand-entered host and port the user confirmed for an alias without HostName or Port", async () => {
    writeSshConfig("Host qa-alias\n");

    await startAliasRegistration();
    await user.type(
      screen.getByLabelText("Hostname or IP address"),
      LOOPBACK_SSH_HOSTNAME,
    );
    await user.clear(screen.getByLabelText("SSH port"));
    await user.type(screen.getByLabelText("SSH port"), "2222");
    await user.click(screen.getByRole("button", { name: "Continue" }));
    expect(
      await screen.findByTestId("host-trust-confirmation"),
    ).toHaveTextContent(`treq@${LOOPBACK_SSH_HOSTNAME}:2222`);
    await user.click(screen.getByRole("button", { name: "Trust and connect" }));

    await screen.findByLabelText("Remote repository path");
    const [record] = await listUserManagedEndpoints();
    expect(record).toMatchObject({
      alias: "qa-alias",
      hostname: LOOPBACK_SSH_HOSTNAME,
      port: 2222,
    });
  }, 20_000);
});
