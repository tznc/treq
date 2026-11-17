import { describe, expect, it, vi } from "vitest";
import { installationLinkStore } from "../../supabase/functions/complete-github-installation/lib.ts";

describe("installationLinkStore.saveUserToken", () => {
  it("saves the grant to Vault through github_user_tokens_save", async () => {
    const rpc = vi.fn(async () => ({ data: null, error: null }));
    const store = installationLinkStore({ rpc, from: vi.fn() });
    await store.saveUserToken("user-1", {
      accessToken: "ghu_a",
      refreshToken: "ghr_r",
      expiresAt: "2026-11-08T08:00:00.000Z",
      refreshExpiresAt: "2027-05-08T00:00:00.000Z",
    });
    expect(rpc).toHaveBeenCalledWith("github_user_tokens_save", {
      p_user_id: "user-1",
      p_access_token: "ghu_a",
      p_refresh_token: "ghr_r",
      p_expires_at: "2026-11-08T08:00:00.000Z",
      p_refresh_expires_at: "2027-05-08T00:00:00.000Z",
    });
  });

  it("throws when the save fails", async () => {
    const rpc = vi.fn(async () => ({
      data: null,
      error: { message: "denied" },
    }));
    const store = installationLinkStore({ rpc, from: vi.fn() });
    await expect(
      store.saveUserToken("user-1", {
        accessToken: "a",
        refreshToken: null,
        expiresAt: null,
        refreshExpiresAt: null,
      }),
    ).rejects.toThrow(/denied/);
  });
});
