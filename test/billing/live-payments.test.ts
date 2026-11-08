import { describe, expect, it } from "vitest";
import pkg from "../../package.json";
import { STRIPE_PUBLISHABLE_KEY_PROD } from "../../web/src/lib/constants";

// Production dashboards open Checkout only when both are set.
describe("live payments", () => {
  it("enables the stripePayments flag", () => {
    expect(pkg.featureFlags.stripePayments).toBe(true);
  });

  it("ships a live Stripe publishable key", () => {
    expect(STRIPE_PUBLISHABLE_KEY_PROD).toMatch(/^pk_live_\w+$/);
  });
});
