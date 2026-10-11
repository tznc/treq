import { describe, expect, it } from "vitest";
import {
  isProSubscription,
  subscriptionPeriodLabel,
  subscriptionStatusLabel,
} from "./subscription";

describe("isProSubscription", () => {
  it.each([
    "trialing",
    "active",
    "past_due",
    "canceled",
  ])("treats pro / %s as Pro", (status) => {
    expect(isProSubscription({ plan: "pro", status })).toBe(true);
  });

  it.each([
    "inactive",
    "unpaid",
    "incomplete",
    "paused",
    "",
  ])("does not treat pro / %s as Pro", (status) => {
    expect(isProSubscription({ plan: "pro", status })).toBe(false);
  });

  it("does not treat the free plan as Pro whatever its status", () => {
    expect(isProSubscription({ plan: "free", status: "active" })).toBe(false);
  });

  it("does not treat a missing subscription as Pro", () => {
    expect(isProSubscription(null)).toBe(false);
    expect(isProSubscription(undefined)).toBe(false);
  });
});

describe("subscription labels", () => {
  it.each([
    ["trialing", "Trial", "Trial ends"],
    ["active", "Active", "Renews on"],
    ["past_due", "Past due", "Renews on"],
    ["canceled", "Canceling", "Pro until"],
    ["inactive", "Inactive", "Period ends"],
  ])("labels %s", (status, statusLabel, periodLabel) => {
    expect(subscriptionStatusLabel(status)).toBe(statusLabel);
    expect(subscriptionPeriodLabel(status)).toBe(periodLabel);
  });
});
