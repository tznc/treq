// @include-parallel
import * as React from "react";
import { beforeEach, describe, expect, it, vi } from "vitest";
import userEvent from "@testing-library/user-event";
import { createTestRepo, openRepo } from "../utils";
import { render, screen, within } from "../test-utils";
import { Dashboard } from "../../src/components/Dashboard";
import { getSetting, setSetting } from "../../src/lib/api";

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

vi.mock("../../src/lib/supabase", async (importOriginal) => {
  const actual =
    await importOriginal<typeof import("../../src/lib/supabase")>();
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

async function openAccountSettings(user: ReturnType<typeof userEvent.setup>) {
  render(<Dashboard />);
  await user.click(await screen.findByLabelText("Settings"));
  await user.click(await screen.findByRole("tab", { name: /account/i }));
  await screen.findByText("ada@example.com");
}

describe("Delete account", () => {
  let user: ReturnType<typeof userEvent.setup>;

  beforeEach(async () => {
    const { repoPath } = createTestRepo(false);
    openRepo(repoPath);
    user = userEvent.setup();
    supabaseMock.invoke.mockReset();
    supabaseMock.signOut.mockClear();
    await setSetting(
      "supabase_session",
      JSON.stringify({ accessToken: "access", refreshToken: "refresh" }),
    );
  });

  it("deletes the account only after the email is typed, then signs out locally", async () => {
    supabaseMock.invoke.mockResolvedValue({
      data: { deleted: true },
      error: null,
    });
    await openAccountSettings(user);

    await user.click(screen.getByRole("button", { name: /delete account/i }));
    const dialog = await screen.findByRole("alertdialog");
    const confirm = within(dialog).getByRole("button", {
      name: /delete account/i,
    });
    expect(confirm).toBeDisabled();

    const input = within(dialog).getByLabelText(/type your email/i);
    await user.type(input, "someone@example.com");
    expect(confirm).toBeDisabled();

    await user.clear(input);
    await user.type(input, "ada@example.com");
    expect(confirm).toBeEnabled();
    await user.click(confirm);

    expect(
      await screen.findByRole("heading", { name: /sign in to treq/i }),
    ).toBeVisible();
    const deleteCalls = supabaseMock.invoke.mock.calls.filter(
      ([name]) => name === "delete-account",
    );
    expect(deleteCalls).toEqual([["delete-account", { body: {} }]]);
    expect(supabaseMock.signOut).toHaveBeenCalledWith({ scope: "local" });
    expect(await getSetting("supabase_session")).toBe("");
  });

  it("keeps the user signed in and shows the server's reason when deletion fails", async () => {
    supabaseMock.invoke.mockResolvedValue({
      data: null,
      error: Object.assign(new Error("Edge Function returned a non-2xx"), {
        context: new Response(
          JSON.stringify({
            error:
              "Your subscription could not be canceled. Your account was not deleted. Try again.",
          }),
          { status: 502 },
        ),
      }),
    });
    await openAccountSettings(user);

    await user.click(screen.getByRole("button", { name: /delete account/i }));
    const dialog = await screen.findByRole("alertdialog");
    await user.type(
      within(dialog).getByLabelText(/type your email/i),
      "ada@example.com",
    );
    await user.click(
      within(dialog).getByRole("button", { name: /delete account/i }),
    );

    expect(await within(dialog).findByRole("alert")).toHaveTextContent(
      /your subscription could not be canceled/i,
    );
    expect(screen.getByText("ada@example.com")).toBeVisible();
    expect(supabaseMock.signOut).not.toHaveBeenCalled();
    expect(await getSetting("supabase_session")).not.toBe("");
  });
});
