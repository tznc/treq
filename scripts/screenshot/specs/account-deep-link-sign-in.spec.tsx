import * as React from "react";
import { expect, it, onTestFinished, vi } from "vitest";
import userEvent from "@testing-library/user-event";
import { listen } from "@tauri-apps/api/event";
import { createTestRepo, openRepo } from "../../../test/utils";
import { render, screen, waitFor } from "../../../test/test-utils";
import { Dashboard } from "../../../src/components/Dashboard";
import { AppStoreEffects } from "../../../src/stores/AppStoreEffects";
import { captureDocument } from "../capture";

/** Delivers a `deep-link-received` event the way the desktop backend does. */
async function openDeepLink(url: string) {
  const handlers = await waitFor(() => {
    const found = vi
      .mocked(listen)
      .mock.calls.filter(([event]) => event === "deep-link-received")
      .map(([, handler]) => handler as (e: { payload: string[] }) => unknown);
    expect(found.length).toBeGreaterThan(0);
    return found;
  });
  for (const handler of handlers) void handler({ payload: [url] });
}

it("captures Account settings while a treq:// sign-in completes", async () => {
  const { repoPath } = createTestRepo(false);
  openRepo(repoPath);

  // Hold the token exchange open so the in-flight state can be captured.
  let finishExchange!: (response: Response) => void;
  const realFetch = globalThis.fetch;
  vi.stubGlobal(
    "fetch",
    vi.fn((input: RequestInfo | URL, init?: RequestInit) =>
      String(input).includes("exchange-desktop-token")
        ? new Promise<Response>((resolve) => {
            finishExchange = resolve;
          })
        : realFetch(input, init),
    ),
  );
  onTestFinished(() => vi.unstubAllGlobals());

  const user = userEvent.setup();
  render(
    <>
      <AppStoreEffects />
      <Dashboard />
    </>,
  );

  await user.click(await screen.findByLabelText("Settings"));
  await user.click(await screen.findByRole("tab", { name: /account/i }));
  await screen.findByRole("button", { name: "Sign in with Browser" });

  await captureDocument(document, {
    name: "account-deep-link-01-signed-out",
    expectations: [
      "The Account settings tab shows 'Sign in to Treq' with a 'Sign in with Browser' button.",
    ],
  });

  await openDeepLink("treq://auth/callback?token=one-time-token");
  expect(await screen.findByRole("status")).toHaveTextContent(
    "Signing you in…",
  );
  expect(
    screen.queryByRole("button", { name: "Sign in with Browser" }),
  ).toBeNull();

  await captureDocument(document, {
    name: "account-deep-link-02-signing-in",
    expectations: [
      "The Account tab shows a spinner with the text 'Signing you in…'.",
      "The 'Sign in with Browser' button is no longer shown.",
    ],
  });

  finishExchange(
    new Response(JSON.stringify({ error: "Token expired" }), { status: 400 }),
  );
  expect(await screen.findByRole("alert")).toHaveTextContent(
    "Sign-in failed: Token expired",
  );

  await captureDocument(document, {
    name: "account-deep-link-03-sign-in-failed",
    expectations: [
      "The 'Sign in with Browser' button is back so the user can retry.",
      "Red text under the button reads 'Sign-in failed: Token expired'.",
    ],
  });
}, 60000);
