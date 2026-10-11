import { describe, expect, it } from "vitest";
import { repoDisplayLabels } from "./repo-labels";

describe("repoDisplayLabels", () => {
  it("uses the directory name", () => {
    expect(repoDisplayLabels(["/src/app", "/src/api/"])).toEqual(
      new Map([
        ["/src/app", "app"],
        ["/src/api/", "api"],
      ]),
    );
  });

  it("adds the parent directory when names collide", () => {
    expect(
      repoDisplayLabels(["/work/org/app", "/work/other/app", "/work/api"]),
    ).toEqual(
      new Map([
        ["/work/org/app", "org/app"],
        ["/work/other/app", "other/app"],
        ["/work/api", "api"],
      ]),
    );
  });

  it("handles Windows separators", () => {
    expect(repoDisplayLabels(["C:\\code\\app"]).get("C:\\code\\app")).toBe(
      "app",
    );
  });
});
