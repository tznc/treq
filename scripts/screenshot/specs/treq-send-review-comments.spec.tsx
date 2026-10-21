/**
 * Visual QA: reviewing treq-send attachments — toolbar comment, image
 * highlights with floating comments, and inline line comments on text assets.
 */

import * as React from "react";
import { expect, it, vi } from "vitest";
import userEvent from "@testing-library/user-event";
import { listen } from "@tauri-apps/api/event";
import {
  createTestRepo,
  openRepo,
} from "../../../test/utils";
import { render, screen, waitFor } from "../../../test/test-utils";
import { Dashboard } from "../../../src/components/Dashboard";
import { createWorkspace } from "../../../src/lib/api";
import { TREQ_SEND_EVENT } from "../../../src/lib/treqSend";
import { captureDocument } from "../capture";
import fs from "node:fs";
import path from "node:path";
import { clickGpuiRow, startGpuiAgent } from "../../../test/gpui-sidebar";

type SendPayload = {
  kind: string;
  request_id: string;
  repo: string;
  pty_session_id: string;
  media_type: string;
  path: string;
  title: string;
};

it("captures attachment review comments in the send lightbox", async () => {
  const { repoPath } = createTestRepo(false);
  openRepo(repoPath);
  await createWorkspace(repoPath, "feat/send-review");

  const notePath = path.join(repoPath, "review-note.txt");
  fs.writeFileSync(notePath, "alpha\nbeta\ngamma\n");
  const imagePath = path.join(repoPath, "review-shot.svg");
  fs.writeFileSync(
    imagePath,
    `<svg xmlns="http://www.w3.org/2000/svg" width="240" height="160">
  <rect width="240" height="160" fill="#1d4ed8"/>
  <rect x="20" y="20" width="80" height="50" fill="#f8fafc"/>
</svg>
`,
  );

  const user = userEvent.setup();
  if (!Element.prototype.setPointerCapture) {
    Element.prototype.setPointerCapture = () => {};
    Element.prototype.releasePointerCapture = () => {};
  }

  render(<Dashboard />);
  await clickGpuiRow("feat/send-review");
  await screen.findByTestId("workspace-terminal-pane");
  await startGpuiAgent("feat/send-review");

  const terminalEl = await waitFor(() => {
    const el = document.querySelector(
      '[data-terminal-id^="agent-"]',
    ) as HTMLElement | null;
    expect(el).not.toBeNull();
    return el as HTMLElement;
  });
  const terminalId = terminalEl.getAttribute("data-terminal-id");
  expect(terminalId).toBeTruthy();
  const ptySessionId = `session-${terminalId!.replace(/^agent-/, "")}`;

  await waitFor(() => {
    expect(
      vi.mocked(listen).mock.calls.some((args) => args[0] === TREQ_SEND_EVENT),
    ).toBe(true);
  });
  const sendCallback = vi
    .mocked(listen)
    .mock.calls.find((args) => args[0] === TREQ_SEND_EVENT)?.[1] as (event: {
    payload: SendPayload;
  }) => void;

  sendCallback({
    payload: {
      kind: "send",
      request_id: "qa-review-text",
      repo: repoPath,
      pty_session_id: ptySessionId!,
      media_type: "text",
      path: notePath,
      title: "review-note.txt",
    },
  });
  sendCallback({
    payload: {
      kind: "send",
      request_id: "qa-review-image",
      repo: repoPath,
      pty_session_id: ptySessionId!,
      media_type: "image",
      path: imagePath,
      title: "review-shot.svg",
    },
  });

  await user.click(
    await screen.findByTestId("terminal-send-preview-qa-review-text"),
  );
  await screen.findByTestId("treq-send-preview-lightbox");
  expect(screen.getByTestId("treq-send-review-input")).toBeTruthy();
  await waitFor(() => {
    expect(screen.getByTestId("treq-send-text-line-2")).toBeTruthy();
  });
  await user.click(screen.getByTestId("treq-send-text-line-2"));
  const composer = await screen.findByTestId("treq-send-line-comment-composer");
  await user.type(
    composer.querySelector("textarea") as HTMLTextAreaElement,
    "Rename beta",
  );
  await user.click(screen.getByTestId("treq-send-line-comment-save"));
  await screen.findByText("Rename beta");

  await captureDocument(document, {
    name: "treq-send-review-01-text-inline",
    expectations: [
      "The lightbox shows review-note.txt with numbered lines and a saved inline comment 'Rename beta' under line 2.",
      "A review comment input sits next to the copy/reveal/close toolbar above the text.",
      "The text panel is fully opaque; only the lightbox backdrop behind it is dimmed.",
    ],
  });

  await user.click(screen.getByTestId("treq-send-close"));
  expect(await screen.findByTestId("treq-send-unsaved-dialog")).toBeTruthy();

  await captureDocument(document, {
    name: "treq-send-review-02-unsaved-warning",
    expectations: [
      "A discard confirmation dialog asks about unsent comments.",
      "The lightbox is still visible behind the dialog.",
      "Keep reviewing and Discard comments actions are shown.",
    ],
  });

  await user.click(screen.getByRole("button", { name: "Keep reviewing" }));
  await waitFor(() => {
    expect(screen.queryByTestId("treq-send-unsaved-dialog")).toBeNull();
  });
  await user.click(screen.getByTestId("treq-send-close"));
  await user.click(await screen.findByTestId("treq-send-unsaved-discard"));
  await waitFor(() => {
    expect(screen.queryByTestId("treq-send-preview-lightbox")).toBeNull();
  });

  await user.hover(
    screen
      .getByTestId("terminal-send-preview-qa-review-text")
      .closest('[data-slot="attachment"]') as HTMLElement,
  );
  await user.click(screen.getByTestId("terminal-send-dismiss-qa-review-text"));
  await waitFor(() => {
    expect(
      screen.queryByTestId("terminal-send-preview-qa-review-text"),
    ).toBeNull();
  });

  await user.click(
    await screen.findByTestId("terminal-send-preview-qa-review-image"),
  );
  await screen.findByTestId("treq-send-preview-lightbox");
  expect(screen.getByTestId("treq-send-preview-header").textContent).toContain(
    "review-shot.svg",
  );
  expect(screen.queryByTestId("treq-send-text-preview")).toBeNull();
  const surface = await screen.findByTestId("treq-send-highlight-surface");
  vi.spyOn(surface, "getBoundingClientRect").mockReturnValue({
    x: 0,
    y: 0,
    top: 0,
    left: 0,
    bottom: 160,
    right: 240,
    width: 240,
    height: 160,
    toJSON: () => ({}),
  });
  await user.pointer([
    { keys: "[MouseLeft>]", target: surface, coords: { x: 24, y: 24 } },
    { target: surface, coords: { x: 100, y: 70 } },
    { keys: "[/MouseLeft]", target: surface, coords: { x: 100, y: 70 } },
  ]);
  const commentBox = await screen.findByLabelText("Highlight comment");
  await user.type(commentBox, "Move this panel");

  await captureDocument(document, {
    name: "treq-send-review-03-image-highlight",
    expectations: [
      "A red-bordered highlight box sits on the blue image.",
      "A floating comment popover next to the highlight contains 'Move this panel'.",
      "The review comment input remains beside the zoom toolbar above the image.",
    ],
  });
}, 60000);
