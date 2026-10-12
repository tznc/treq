import { describe, expect, it } from "vitest";
import { isAllowedBrowserUrl } from "./utils";

describe("isAllowedBrowserUrl", () => {
  it("allows localhost URLs with any port", () => {
    expect(isAllowedBrowserUrl("http://localhost:3000")).toBe(true);
    expect(isAllowedBrowserUrl("http://localhost:5173/app?x=1")).toBe(true);
  });

  it("allows 127.0.0.1 URLs", () => {
    expect(isAllowedBrowserUrl("http://127.0.0.1:8080/")).toBe(true);
  });

  it("allows file:// URLs", () => {
    expect(isAllowedBrowserUrl("file:///tmp/preview.html")).toBe(true);
  });

  it("rejects general internet URLs", () => {
    expect(isAllowedBrowserUrl("https://example.com")).toBe(false);
    expect(isAllowedBrowserUrl("http://example.com")).toBe(false);
    expect(isAllowedBrowserUrl("http://192.168.1.5:3000")).toBe(false);
  });

  it("rejects malformed input", () => {
    expect(isAllowedBrowserUrl("not a url")).toBe(false);
    expect(isAllowedBrowserUrl("")).toBe(false);
  });
});
