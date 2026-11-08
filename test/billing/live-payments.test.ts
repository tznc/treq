import { describe, expect, it } from "vitest";
import pkg from "../../package.json";

// Production dashboards open Checkout only with this flag on.
describe("live payments", () => {
  it("enables the stripePayments flag", () => {
    expect(pkg.featureFlags.stripePayments).toBe(true);
  });
});
