import * as React from "react";
import { expect, it, vi } from "vitest";
import userEvent from "@testing-library/user-event";
import { createTestRepo, openRepo } from "../../../test/utils";
import { render, screen, within } from "../../../test/test-utils";
import { Dashboard } from "../../../src/components/Dashboard";
import { setSetting } from "../../../src/lib/api";
import { captureDocument } from "../capture";

// Supabase is the network boundary: the account deletion itself is verified
// against the local stack by scripts/service-qa/specs/delete-account.spec.ts.
// Here only the client is stubbed; settings storage runs through real NAPI.
const supabaseMock = vi.hoisted(() => {
  const user = {
    id: "user-1",
    email: "ada@example.com",
    user_metadata: { full_name: "Ada Lovelace" },
  };
  return {
    user,
    invoke: vi.fn(),
    signOut: vi.fn(async () => ({ error: null })),
    setSession: vi.fn(async () => ({
      data: {
        session: {
          access_token: "access",
          refresh_token: "refresh",
          user,
        },
      },
      error: null,
    })),
  };
});

vi.mock("../../../src/lib/supabase", async (importOriginal) => {
  const actual =
    await importOriginal<typeof import("../../../src/lib/supabase")>();
  const auth = new Proxy(actual.supabase.auth, {
    get(target, prop, receiver) {
      if (prop === "setSession") return supabaseMock.setSession;
      if (prop === "signOut") return supabaseMock.signOut;
      return Reflect.get(target, prop, receiver);
    },
  });
  const supabase = new Proxy(actual.supabase, {
    get(target, prop, receiver) {
      if (prop === "auth") return auth;
      if (prop === "functions") return { invoke: supabaseMock.invoke };
      if (prop === "from") {
        return () => ({
          select: () => ({
            single: async () => ({ data: null, error: { message: "offline" } }),
          }),
        });
      }
      return Reflect.get(target, prop, receiver);
    },
  });
  return { ...actual, supabase };
});

it("captures the delete account confirmation, failure and success", async () => {
  const { repoPath } = createTestRepo(false);
  openRepo(repoPath);
  await setSetting(
    "supabase_session",
    JSON.stringify({ accessToken: "access", refreshToken: "refresh" }),
  );
  supabaseMock.invoke.mockImplementation(async (name: string) =>
    name === "delete-account"
      ? {
          data: null,
          error: Object.assign(new Error("non-2xx"), {
            context: new Response(
              JSON.stringify({
                error:
                  "Your subscription could not be canceled. Your account was not deleted. Try again.",
              }),
              { status: 502 },
            ),
          }),
        }
      : { data: { instance: null, endpoint: null }, error: null },
  );

  const user = userEvent.setup();
  render(<Dashboard />);
  await user.click(await screen.findByLabelText("Settings"));
  await user.click(await screen.findByRole("tab", { name: /account/i }));
  await screen.findByText("ada@example.com");

  await user.click(screen.getByRole("button", { name: /delete account/i }));
  const dialog = await screen.findByRole("alertdialog");
  const confirm = within(dialog).getByRole("button", {
    name: /delete account/i,
  });
  expect(confirm).toBeDisabled();
  await captureDocument(document, {
    name: "delete-account-01-confirm-empty",
    expectations: [
      "A dialog titled 'Delete your Treq account?' explains the subscription, cloud workspace and integrations consequences.",
      "An empty input is labelled 'Type your email (ada@example.com) to confirm'.",
      "The red 'Delete Account' button in the dialog footer is visibly disabled.",
    ],
  });

  await user.type(
    within(dialog).getByLabelText(/type your email/i),
    "ada@example.com",
  );
  expect(confirm).toBeEnabled();
  await user.click(confirm);
  await within(dialog).findByRole("alert");
  await captureDocument(document, {
    name: "delete-account-02-failed",
    expectations: [
      "The input holds 'ada@example.com' and the dialog stays open.",
      "Red text reads 'Your subscription could not be canceled. Your account was not deleted. Try again.'",
    ],
  });

  supabaseMock.invoke.mockImplementation(async (name: string) =>
    name === "delete-account"
      ? { data: { deleted: true }, error: null }
      : { data: { instance: null, endpoint: null }, error: null },
  );
  await user.click(confirm);
  await screen.findByRole("heading", { name: /sign in to treq/i });
  await captureDocument(document, {
    name: "delete-account-03-signed-out",
    expectations: [
      "The dialog is gone and the Account tab shows 'Sign in to Treq' with a 'Sign in with Browser' button.",
    ],
  });
}, 60000);
