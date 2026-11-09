import { describe, expect, it } from "vitest";
import pkg from "../../package.json";
import { STRIPE_PUBLISHABLE_KEY_PROD } from "../../web/src/lib/constants";

// Payments are live: no stripePayments flag gates checkout, and production
// builds carry the live publishable key.
describe("live payments", () => {
  it("has no stripePayments flag", () => {
    expect(pkg.featureFlags).not.toHaveProperty("stripePayments");
  });

  it("ships a live Stripe publishable key", () => {
    expect(STRIPE_PUBLISHABLE_KEY_PROD).toMatch(/^pk_live_\w+$/);
  });
});
