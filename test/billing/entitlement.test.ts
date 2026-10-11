import { describe, expect, it, vi } from "vitest";
import {
  installationHasPro,
  PRO_REQUIRED_CODE,
  proRequired,
  refuseGatedAction,
  userHasPro,
} from "../../supabase/functions/_shared/billing/entitlement.ts";

function rpcReturning(result: {
  data: unknown;
  error: { message: string } | null;
}) {
  return { rpc: vi.fn(async () => result) };
}

describe("proRequired", () => {
  it("answers 402 with the pro_required code and a message naming the feature", () => {
    const result = proRequired("Cloud workspaces need Pro.");
    expect(result.status).toBe(402);
    expect(result.body).toEqual({
      error: "Cloud workspaces need Pro.",
      code: PRO_REQUIRED_CODE,
    });
    expect(PRO_REQUIRED_CODE).toBe("pro_required");
  });
});

describe("refuseGatedAction", () => {
  const gated = new Set(["ensure", "wake"]);

  it("refuses a gated action without Pro", async () => {
    await expect(
      refuseGatedAction("wake", gated, async () => false, "Needs Pro."),
    ).resolves.toEqual(proRequired("Needs Pro."));
  });

  it("lets a gated action through with Pro", async () => {
    await expect(
      refuseGatedAction("wake", gated, async () => true, "Needs Pro."),
    ).resolves.toBeNull();
  });

  it("never asks about an action that is not gated", async () => {
    const hasPro = vi.fn(async () => false);
    await expect(
      refuseGatedAction("status", gated, hasPro, "Needs Pro."),
    ).resolves.toBeNull();
    expect(hasPro).not.toHaveBeenCalled();
  });
});

describe("userHasPro", () => {
  it("asks has_pro about the user", async () => {
    const client = rpcReturning({ data: true, error: null });
    await expect(userHasPro(client, "user-1")).resolves.toBe(true);
    expect(client.rpc).toHaveBeenCalledWith("has_pro", { p_user_id: "user-1" });
  });

  it("treats anything but true as not entitled", async () => {
    await expect(
      userHasPro(rpcReturning({ data: null, error: null }), "user-1"),
    ).resolves.toBe(false);
  });

  it("fails closed by throwing when the check itself fails", async () => {
    await expect(
      userHasPro(
        rpcReturning({ data: null, error: { message: "boom" } }),
        "user-1",
      ),
    ).rejects.toThrow("has_pro failed: boom");
  });
});

describe("installationHasPro", () => {
  it("asks installation_has_pro about the installation", async () => {
    const client = rpcReturning({ data: false, error: null });
    await expect(installationHasPro(client, 42)).resolves.toBe(false);
    expect(client.rpc).toHaveBeenCalledWith("installation_has_pro", {
      p_installation_id: 42,
    });
  });

  it("fails closed by throwing when the check itself fails", async () => {
    await expect(
      installationHasPro(
        rpcReturning({ data: null, error: { message: "down" } }),
        42,
      ),
    ).rejects.toThrow("installation_has_pro failed: down");
  });
});
