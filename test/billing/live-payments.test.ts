import { describe, expect, it } from "vitest";
import { STRIPE_PUBLISHABLE_KEY_PROD } from "../../web/src/lib/constants";

// Production builds open Checkout only when they carry the live publishable key.
describe("live payments", () => {
  it("ships a live Stripe publishable key", () => {
    expect(STRIPE_PUBLISHABLE_KEY_PROD).toMatch(/^pk_live_\w+$/);
  });
});
