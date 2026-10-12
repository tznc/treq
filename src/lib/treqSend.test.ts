import { describe, expect, it } from "vitest";
import {
  assetsForPtySession,
  normalizeSendMediaType,
  parseTreqSendPayload,
} from "./treqSend";

describe("treqSend helpers", () => {
  it("parses send payloads and ignores malformed ones", () => {
    expect(
      parseTreqSendPayload({
        kind: "send",
        request_id: "abc",
        repo: "/repo",
        media_type: "image",
        path: "/repo/a.png",
        title: "a.png",
      }),
    ).toMatchObject({
      id: "abc",
      mediaType: "image",
      path: "/repo/a.png",
      title: "a.png",
    });
    expect(parseTreqSendPayload({ request_id: "x" })).toBeNull();
  });

  it("normalizes the browser media type from treq send --browser", () => {
    expect(normalizeSendMediaType("browser")).toBe("browser");
    expect(
      parseTreqSendPayload({
        kind: "send",
        request_id: "b1",
        repo: "/repo",
        media_type: "browser",
        path: "http://localhost:3000",
        title: "http://localhost:3000",
      }),
    ).toMatchObject({
      id: "b1",
      mediaType: "browser",
      path: "http://localhost:3000",
    });
  });

  it("filters assets to the matching pty session", () => {
    const assets = [
      {
        id: "1",
        repo: "/r",
        ptySessionId: "session-a",
        mediaType: "text" as const,
        path: "/r/a.txt",
        title: "a.txt",
        receivedAt: 1,
      },
      {
        id: "2",
        repo: "/r",
        ptySessionId: null,
        mediaType: "text" as const,
        path: "/r/b.txt",
        title: "b.txt",
        receivedAt: 2,
      },
    ];
    expect(
      assetsForPtySession(assets, "session-a", false).map((a) => a.id),
    ).toEqual(["1"]);
    expect(
      assetsForPtySession(assets, "session-b", true).map((a) => a.id),
    ).toEqual(["2"]);
  });
});
