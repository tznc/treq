import * as React from "react";
import userEvent from "@testing-library/user-event";
import * as fs from "node:fs";
import * as os from "node:os";
import * as path from "node:path";
import { it, onTestFinished } from "vitest";
import { Dashboard } from "../../../src/components/Dashboard";
import { render, screen } from "../../../test/test-utils";
import {
  LOOPBACK_SSH_HOSTNAME,
  TEST_SSH_PUBLIC_KEY,
} from "../../../test/loopback-ssh";
import { captureDocument } from "../capture";

// The key picker lists ~/.ssh/*.pub, so give the run one key to select.
function useFixtureSshKey() {
  const home = fs.mkdtempSync(path.join(os.tmpdir(), "treq-ssh-home-"));
  fs.mkdirSync(path.join(home, ".ssh"));
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

it("captures the remote setup flow opening from onboarding", async () => {
  window.history.replaceState({}, "", "/");
  useFixtureSshKey();
  const user = userEvent.setup();
  render(<Dashboard />);

  await screen.findByRole("button", { name: "Open via SSH" });
  await captureDocument(document, {
    name: "remote-ssh-dialog-01-onboarding",
    expectations: [
      "An onboarding card is shown with an 'Open Repository' button and an 'Open via SSH' button.",
    ],
  });

  await user.click(await screen.findByRole("button", { name: "Open via SSH" }));
  await screen.findByRole("dialog", { name: "Connect a remote repository" });
  await captureDocument(document, {
    name: "remote-ssh-dialog-02-choice",
    expectations: [
      "A modal dialog titled 'Connect a remote repository' is shown, overlaying the onboarding card, offering 'Treq-managed cloud workspace' and 'Your own VM' choices.",
    ],
  });

  await user.click(await screen.findByRole("button", { name: /Your own VM/ }));
  await screen.findByDisplayValue(/id_ed25519/);
  await captureDocument(document, {
    name: "remote-ssh-dialog-04-user-managed",
    expectations: [
      "The dialog now shows the 'Your own VM' form with Display name, Hostname or IP address, SSH port, Username, and an 'SSH key' picker with id_ed25519 selected. There is no fingerprint field, and Continue is disabled.",
    ],
  });

  await user.type(screen.getByLabelText("Display name"), "My dev box");
  await user.type(
    screen.getByLabelText("Hostname or IP address"),
    LOOPBACK_SSH_HOSTNAME,
  );
  await user.type(screen.getByLabelText("Username"), "dev");
  await user.click(screen.getByRole("button", { name: "Continue" }));
  await screen.findByTestId("host-trust-confirmation");
  await captureDocument(document, {
    name: "remote-ssh-dialog-05-host-trust-confirmation",
    expectations: [
      "A yellow host-trust confirmation box is shown, summarizing the user@host:port and the SHA256:loopback fingerprint the host presented, with 'Trust and connect' and 'Cancel' buttons.",
    ],
  });
}, 60000);
