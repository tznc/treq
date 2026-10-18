import { describe, expect, it } from "vitest";
import { githubPrReviewPath, parsePrReviewPath } from "./githubRoutes";

describe("PR review route", () => {
  it("round-trips a PR number and filter", () => {
    expect(githubPrReviewPath(42, "closed")).toBe(
      "/github/prs/closed/42/review",
    );
    expect(parsePrReviewPath("/github/prs/closed/42/review")).toEqual({
      number: 42,
      filter: "closed",
    });
  });

  it("ignores other GitHub routes and bad filters", () => {
    expect(parsePrReviewPath("/github/prs/open/42")).toBeNull();
    expect(parsePrReviewPath("/github/prs/open/x/review")).toBeNull();
    expect(parsePrReviewPath("/github/prs/bogus/4/review")).toEqual({
      number: 4,
      filter: "open",
    });
  });
});
